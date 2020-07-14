// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use getset::Getters;
use hyper::{Body, Request, Response};
use hyper::service::{make_service_fn, service_fn};
use riker::actors::ActorSystem;
use slog::{Logger, warn};
use futures::lock::Mutex;

use crypto::hash::{BlockHash, HashType};
use storage::persistent::PersistentStorage;
use shell::shell_channel::ShellChannelRef;

use tezos_api::environment::TezosEnvironmentConfiguration;
use tezos_wrapper::service::{IpcCmdServer, ProtocolController, ProtocolServiceError};

use crate::empty;
use crate::rpc_actor::{RpcCollectedStateRef, RpcServerRef};

mod handler;
mod dev_handler;
mod service;
mod service_stats;
mod router;

/// Server environment parameters
#[derive(Getters, Clone)]
pub struct RpcServiceEnvironment {
    #[get = "pub(crate)"]
    sys: ActorSystem,
    #[get = "pub(crate)"]
    actor: RpcServerRef,
    #[get = "pub(crate)"]
    persistent_storage: PersistentStorage,
    #[get = "pub(crate)"]
    genesis_hash: String,
    #[get = "pub(crate)"]
    state: RpcCollectedStateRef,
    #[get = "pub(crate)"]
    shell_channel: ShellChannelRef,
    #[get = "pub(crate)"]
    tezos_environment: TezosEnvironmentConfiguration,
    #[get = "pub(crate)"]
    log: Logger,
}

impl RpcServiceEnvironment {
    pub fn new(sys: ActorSystem, actor: RpcServerRef, shell_channel: ShellChannelRef, tezos_environment: &TezosEnvironmentConfiguration, persistent_storage: &PersistentStorage, genesis_hash: &BlockHash, state: RpcCollectedStateRef, log: &Logger) -> Self {
        Self { sys, actor, shell_channel: shell_channel.clone(), tezos_environment: tezos_environment.clone(), persistent_storage: persistent_storage.clone(), genesis_hash: HashType::BlockHash.bytes_to_string(genesis_hash), state, log: log.clone() }
    }
}

pub type Params = Vec<(String, String)>;

pub type Query = HashMap<String, Vec<String>>;

pub type HResult = Result<Response<Body>, Box<dyn std::error::Error + Sync + Send>>;

pub type Handler = Arc<dyn Fn(Request<Body>, Params, Query, RpcServiceEnvironment, ProtocolController) -> Box<dyn Future<Output = HResult> + Send> + Send + Sync>;


/// Spawn new HTTP server on given address interacting with specific actor system
pub fn spawn_server(bind_address: &SocketAddr, env: RpcServiceEnvironment, (ipc_server, endpoint_name): (IpcCmdServer, String)) -> impl Future<Output=Result<(), hyper::Error>> {
    let routes = Arc::new(router::create_routes());
    let ipc_server = Arc::new(Mutex::new(ipc_server));
    let endpoint_name = Arc::new(endpoint_name);
    let rpc_router_run = Arc::new(AtomicBool::new(true));

    hyper::Server::bind(bind_address)
        .serve(make_service_fn(move |_| {
            let env = env.clone();
            let routes = routes.clone();
            let ipc_server = ipc_server.clone();
            let rpc_router_run = rpc_router_run.clone();
            let endpoint_name = endpoint_name.clone();

            async move {
                let env = env.clone();
                let routes = routes.clone();
                let rpc_router_run = rpc_router_run.clone();
                let ipc_server = ipc_server.clone();
                let endpoint_name = endpoint_name.clone();
                Ok::<_, hyper::Error>(service_fn(move |req: Request<Body>| {
                    let env = env.clone();
                    let routes = routes.clone();
                    let rpc_router_run = rpc_router_run.clone();
                    let ipc_server = ipc_server.clone();
                    let endpoint_name = endpoint_name.clone();

                    async move {
                        let rpc_router_run = rpc_router_run.clone();
                        let inner_log = env.sys().log();
                        let sys = env.sys().clone();
                        let tezos_env = env.tezos_environment().clone();
                        let ipc_server = ipc_server;
                        let endpoint_name = endpoint_name.clone();
                        
                        while rpc_router_run.load(Ordering::Acquire) {
                            match ipc_server.lock().await.accept() {
                                Ok(protocol_controller) => {
                                    if let Some((handler, params)) = routes.find(&req.uri().path().to_string()) {
                                        let params: Params = params.into_iter().map(|(param, value)| (param.to_string(), value.to_string())).collect();
                                        let query: Query = req.uri().query().map(parse_query_string).unwrap_or_else(|| HashMap::new());

                                        let handler = handler.clone();
                                        let fut = handler(req, params, query, env, protocol_controller);
                                        return Pin::from(fut).await
                                    } else {
                                        return empty()
                                    }
                                }
                                Err(err) => warn!(inner_log, "RPC router - no connection from protocol runner"; "endpoint" => endpoint_name.clone(), "reason" => format!("{:?}", err)),
                            }
                        }
                        empty()
                    }
                }))
            }
        }))
}

/// Helper for parsing URI queries.
/// Functions takes URI query in format `key1=val1&key1=val2&key2=val3`
/// and produces map `{ key1: [val1, val2], key2: [val3] }`
fn parse_query_string(query: &str) -> HashMap<String, Vec<String>> {
    let mut ret: HashMap<String, Vec<String>> = HashMap::new();
    for (key, value) in query.split('&').map(|x| {
        let mut parts = x.split('=');
        (parts.next().unwrap(), parts.next().unwrap_or(""))
    }) {
        if let Some(vals) = ret.get_mut(key) {
            // append value to existing vector
            vals.push(value.to_string());
        } else {
            // create new vector with a single value
            ret.insert(key.to_string(), vec![value.to_string()]);
        }
    }
    ret
}

pub trait HasSingleValue {
    fn get_str(&self, key: &str) -> Option<&str>;

    fn get_u64(&self, key: &str) -> Option<u64> {
        self.get_str(key).and_then(|value| value.parse::<u64>().ok())
    }

    fn get_usize(&self, key: &str) -> Option<usize> {
        self.get_str(key).and_then(|value| value.parse::<usize>().ok())
    }

    fn contains_key(&self, key: &str) -> bool {
        self.get_str(key).is_some()
    }
}

impl HasSingleValue for Params {
    fn get_str(&self, key: &str) -> Option<&str> {
        self.iter().find_map(|(k, v)| {
            if k == key {
                Some(v.as_str())
            } else {
                None
            }
        })
    }
}

impl HasSingleValue for Query {
    fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).map(|values| values.iter().next().map(String::as_str)).flatten()
    }
}
