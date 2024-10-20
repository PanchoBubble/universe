// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use log::trace;
use log::{debug, error, info, warn};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{read_dir, remove_dir_all, remove_file};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};
use tari_common::configuration::Network;
use tari_common_types::tari_address::TariAddress;
use tari_core::transactions::tari_amount::MicroMinotari;
use tari_shutdown::Shutdown;
use tauri::async_runtime::block_on;
use tauri::{Manager, RunEvent, UpdaterEvent};
use tokio::sync::RwLock;

use app_config::AppConfig;
use app_in_memory_config::{AirdropInMemoryConfig, AppInMemoryConfig};
use binary_resolver::{Binaries, BinaryResolver};
use gpu_miner_adapter::{GpuMinerStatus, GpuNodeSource};
use node_manager::NodeManagerError;
use progress_tracker::ProgressTracker;
use setup_status_event::SetupStatusEvent;
use systemtray_manager::{SystemtrayManager, SystrayData};
use telemetry_manager::TelemetryManager;
use wallet_manager::WalletManagerError;

use crate::cpu_miner::CpuMiner;
use crate::feedback::Feedback;
use crate::gpu_miner::GpuMiner;
use crate::internal_wallet::InternalWallet;
use crate::mm_proxy_manager::{MmProxyManager, StartConfig};
use crate::node_manager::NodeManager;
use crate::p2pool::models::Stats;
use crate::p2pool_manager::{P2poolConfig, P2poolManager};
use crate::wallet_adapter::WalletBalance;
use crate::wallet_manager::WalletManager;
use crate::xmrig_adapter::XmrigAdapter;

mod app_config;
mod app_in_memory_config;
mod binary_resolver;
mod consts;
mod cpu_miner;
mod download_utils;
mod feedback;
mod format_utils;
mod github;
mod gpu_miner;
mod hardware_monitor;
mod internal_wallet;
mod mm_proxy_adapter;
mod mm_proxy_manager;
mod network_utils;
mod node_adapter;
mod node_manager;
mod p2pool;
mod p2pool_adapter;
mod p2pool_manager;
mod process_adapter;
mod process_killer;
mod process_utils;
mod process_watcher;
mod systemtray_manager;
mod telemetry_manager;
mod tests;
mod user_listener;
mod wallet_adapter;
mod wallet_manager;
mod xmrig;
mod xmrig_adapter;
use hardware_monitor::{HardwareMonitor, HardwareParameters};
mod gpu_miner_adapter;
mod progress_tracker;
mod setup_status_event;

const MAX_ACCEPTABLE_COMMAND_TIME: Duration = Duration::from_secs(1);

const LOG_TARGET: &str = "tari::universe::main";
const LOG_TARGET_WEB: &str = "tari::universe::web";

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UpdateProgressRustEvent {
    chunk_length: usize,
    content_length: Option<u64>,
    downloaded: u64,
}

async fn stop_all_miners(state: UniverseAppState, sleep_secs: u64) -> Result<(), String> {
    state
        .cpu_miner
        .write()
        .await
        .stop()
        .await
        .map_err(|e| e.to_string())?;
    state
        .gpu_miner
        .write()
        .await
        .stop()
        .await
        .map_err(|e| e.to_string())?;
    let exit_code = state
        .wallet_manager
        .stop()
        .await
        .map_err(|e| e.to_string())?;
    info!(target: LOG_TARGET, "Wallet manager stopped with exit code: {}", exit_code);
    state
        .mm_proxy_manager
        .stop()
        .await
        .map_err(|e| e.to_string())?;
    let exit_code = state.node_manager.stop().await.map_err(|e| e.to_string())?;
    info!(target: LOG_TARGET, "Node manager stopped with exit code: {}", exit_code);
    let exit_code = state
        .p2pool_manager
        .stop()
        .await
        .map_err(|e| e.to_string())?;
    info!(target: LOG_TARGET, "P2Pool manager stopped with exit code: {}", exit_code);

    state.shutdown.clone().trigger();

    // TODO: Find a better way of knowing that all miners have stopped
    sleep(std::time::Duration::from_secs(sleep_secs));
    Ok(())
}

#[tauri::command]
async fn set_mode(mode: String, state: tauri::State<'_, UniverseAppState>) -> Result<(), String> {
    let timer = Instant::now();
    state
        .config
        .write()
        .await
        .set_mode(mode)
        .await
        .inspect_err(|e| error!(target: LOG_TARGET, "error at set_mode {:?}", e))
        .map_err(|e| e.to_string())?;
    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET, "set_mode took too long: {:?}", timer.elapsed());
    }

    Ok(())
}

#[tauri::command]
async fn send_feedback(
    feedback: String,
    include_logs: bool,
    _window: tauri::Window,
    state: tauri::State<'_, UniverseAppState>,
    _app: tauri::AppHandle,
) -> Result<(), String> {
    let timer = Instant::now();
    state
        .feedback
        .read()
        .await
        .send_feedback(feedback, include_logs)
        .await
        .inspect_err(|e| error!("error at send_feedback {:?}", e))
        .map_err(|e| e.to_string())?;
    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET, "send_feedback took too long: {:?}", timer.elapsed());
    }
    Ok(())
}

#[tauri::command]
async fn set_allow_telemetry(
    allow_telemetry: bool,
    _window: tauri::Window,
    state: tauri::State<'_, UniverseAppState>,
    _app: tauri::AppHandle,
) -> Result<(), String> {
    state
        .config
        .write()
        .await
        .set_allow_telemetry(allow_telemetry)
        .await
        .inspect_err(|e| error!(target: LOG_TARGET, "error at set_allow_telemetry {:?}", e))
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn get_app_id(
    _window: tauri::Window,
    state: tauri::State<'_, UniverseAppState>,
    _app: tauri::AppHandle,
) -> Result<String, ()> {
    let timer = Instant::now();
    let app_id = state.config.read().await.anon_id().to_string();
    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET, "get_app_id took too long: {:?}", timer.elapsed());
    }
    Ok(app_id)
}

#[tauri::command]
async fn set_airdrop_access_token(
    token: String,
    _window: tauri::Window,
    state: tauri::State<'_, UniverseAppState>,
    _app: tauri::AppHandle,
) -> Result<(), String> {
    let timer = Instant::now();
    let mut write_lock = state.airdrop_access_token.write().await;
    *write_lock = Some(token);
    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET,
            "set_airdrop_access_token took too long: {:?}",
            timer.elapsed()
        );
    }
    Ok(())
}

#[tauri::command]
async fn get_app_in_memory_config(
    _window: tauri::Window,
    state: tauri::State<'_, UniverseAppState>,
    _app: tauri::AppHandle,
) -> Result<AirdropInMemoryConfig, ()> {
    let timer = Instant::now();
    let res = state.in_memory_config.read().await.clone().into();
    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET,
            "get_app_in_memory_config took too long: {:?}",
            timer.elapsed()
        );
    }
    Ok(res)
}

#[tauri::command]
async fn set_monero_address(
    monero_address: String,
    state: tauri::State<'_, UniverseAppState>,
) -> Result<(), String> {
    let timer = Instant::now();
    let mut app_config = state.config.write().await;
    app_config
        .set_monero_address(monero_address)
        .await
        .map_err(|e| e.to_string())?;
    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET, "set_monero_address took too long: {:?}", timer.elapsed());
    }
    Ok(())
}

#[tauri::command]
async fn restart_application(
    _window: tauri::Window,
    _state: tauri::State<'_, UniverseAppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // This restart doesn't need to shutdown all the miners
    app.restart();
    Ok(())
}

#[tauri::command]
async fn exit_application(
    _window: tauri::Window,
    state: tauri::State<'_, UniverseAppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    stop_all_miners(state.inner().clone(), 5).await?;

    app.exit(0);
    Ok(())
}

#[tauri::command]
async fn setup_application(
    window: tauri::Window,
    state: tauri::State<'_, UniverseAppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let timer = Instant::now();
    setup_inner(window, state.clone(), app).await.map_err(|e| {
        warn!(target: LOG_TARGET, "Error setting up application: {:?}", e);
        e.to_string()
    })?;

    let res = state.config.read().await.auto_mining();
    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET, "setup_application took too long: {:?}", timer.elapsed());
    }
    Ok(res)
}

#[allow(clippy::too_many_lines)]
async fn setup_inner(
    window: tauri::Window,
    state: tauri::State<'_, UniverseAppState>,
    app: tauri::AppHandle,
) -> Result<(), anyhow::Error> {
    window
        .emit(
            "message",
            SetupStatusEvent {
                event_type: "setup_status".to_string(),
                title: "starting-up".to_string(),
                title_params: None,
                progress: 0.0,
            },
        )
        .inspect_err(|e| error!(target: LOG_TARGET, "Could not emit event 'message': {:?}", e))?;
    let data_dir = app.path_resolver().app_local_data_dir().unwrap();
    let cache_dir = app.path_resolver().app_cache_dir().unwrap();
    let config_dir = app.path_resolver().app_config_dir().unwrap();
    let log_dir = app.path_resolver().app_log_dir().unwrap();

    let cpu_miner_config = state.cpu_miner_config.read().await;
    let mm_proxy_manager = state.mm_proxy_manager.clone();

    let progress = ProgressTracker::new(window.clone());

    let last_binaries_update_timestamp = state.config.read().await.last_binaries_update_timestamp();
    let now = SystemTime::now();

    state
        .telemetry_manager
        .write()
        .await
        .initialize(state.airdrop_access_token.clone(), window.clone())
        .await?;

    BinaryResolver::current()
        .read_current_highest_version(Binaries::MinotariNode, progress.clone())
        .await?;
    BinaryResolver::current()
        .read_current_highest_version(Binaries::MergeMiningProxy, progress.clone())
        .await?;
    BinaryResolver::current()
        .read_current_highest_version(Binaries::Wallet, progress.clone())
        .await?;
    BinaryResolver::current()
        .read_current_highest_version(Binaries::ShaP2pool, progress.clone())
        .await?;
    BinaryResolver::current()
        .read_current_highest_version(Binaries::GpuMiner, progress.clone())
        .await?;

    if now
        .duration_since(last_binaries_update_timestamp)
        .unwrap_or(Duration::from_secs(0))
        > Duration::from_secs(60 * 10)
    // 10 minutes
    {
        let _unused = state
            .config
            .write()
            .await
            .set_last_binaries_update_timestamp(now)
            .await.inspect_err(|e| error!(target: LOG_TARGET, "Could not set last binaries update timestamp: {:?}", e));

        progress.set_max(10).await;
        progress
            .update("checking-latest-version-node".to_string(), None, 0)
            .await;
        let _unused = BinaryResolver::current()
            .ensure_latest(Binaries::MinotariNode, progress.clone())
            .await.inspect_err(|e| error!(target: LOG_TARGET, "Could not ensure latest version of MinotariNode: {:?}", e));
        sleep(Duration::from_secs(1));

        progress.set_max(15).await;
        progress
            .update("checking-latest-version-mmproxy".to_string(), None, 0)
            .await;
        sleep(Duration::from_secs(1));
        let _unused = BinaryResolver::current()
            .ensure_latest(Binaries::MergeMiningProxy, progress.clone())
            .await.inspect_err(|e| error!(target: LOG_TARGET, "Could not ensure latest version of MergeMiningProxy: {:?}", e));
        progress.set_max(20).await;
        progress
            .update("checking-latest-version-wallet".to_string(), None, 0)
            .await;
        sleep(Duration::from_secs(1));
        let _unused = BinaryResolver::current()
            .ensure_latest(Binaries::Wallet, progress.clone())
            .await.inspect_err(|e| error!(target: LOG_TARGET, "Could not ensure latest version of Wallet: {:?}", e));

        sleep(Duration::from_secs(1));
        progress.set_max(25).await;
        progress
            .update("checking-latest-version-gpuminer".to_string(), None, 0)
            .await;
        let _unused = BinaryResolver::current()
            .ensure_latest(Binaries::GpuMiner, progress.clone())
            .await.inspect_err(|e| error!(target: LOG_TARGET, "Could not ensure latest version of GpuMiner: {:?}", e));

        state.gpu_miner.write().await.detect().await?;

        progress.set_max(30).await;
        progress
            .update("checking-latest-version-xmrig".to_string(), None, 0)
            .await;
        sleep(Duration::from_secs(1));
        let _unused = XmrigAdapter::ensure_latest(cache_dir, false, progress.clone())
            .await
            .inspect_err(
                |e| error!(target: LOG_TARGET, "Could not ensure latest version of Xmrig: {:?}", e),
            );

        progress.set_max(35).await;
        progress
            .update("checking-latest-version-sha-p2pool".to_string(), None, 0)
            .await;
        sleep(Duration::from_secs(1));
        let _unused = BinaryResolver::current()
            .ensure_latest(Binaries::ShaP2pool, progress.clone())
            .await.inspect_err(|e| error!(target: LOG_TARGET, "Could not ensure latest version of ShaP2pool: {:?}", e));
    }

    for _i in 0..2 {
        match state
            .node_manager
            .ensure_started(
                state.shutdown.to_signal(),
                data_dir.clone(),
                config_dir.clone(),
                log_dir.clone(),
            )
            .await
        {
            Ok(_) => {}
            Err(e) => {
                if let NodeManagerError::ExitCode(code) = e {
                    if code == 114 {
                        warn!(target: LOG_TARGET, "Database for node is corrupt or needs a reset, deleting and trying again.");
                        state.node_manager.clean_data_folder(&data_dir).await?;
                        continue;
                    }
                }
                error!(target: LOG_TARGET, "Could not start node manager: {:?}", e);

                app.exit(-1);
                return Err(e.into());
            }
        }
    }

    info!(target: LOG_TARGET, "Node has started and is ready");

    progress.set_max(40).await;
    progress
        .update("waiting-for-wallet".to_string(), None, 0)
        .await;
    state
        .wallet_manager
        .ensure_started(
            state.shutdown.to_signal(),
            data_dir.clone(),
            config_dir.clone(),
            log_dir.clone(),
        )
        .await?;

    progress.set_max(75).await;
    progress
        .update("preparing-for-initial-sync".to_string(), None, 0)
        .await;
    state.node_manager.wait_synced(progress.clone()).await?;

    if state.config.read().await.p2pool_enabled() {
        progress.set_max(85).await;
        progress
            .update("starting-p2pool".to_string(), None, 0)
            .await;

        let base_node_grpc = state.node_manager.get_grpc_port().await?;
        let p2pool_config = P2poolConfig::builder()
            .with_base_node(base_node_grpc)
            .build()?;

        state
            .p2pool_manager
            .ensure_started(
                state.shutdown.to_signal(),
                p2pool_config,
                data_dir,
                config_dir,
                log_dir,
            )
            .await?;
    }

    progress.set_max(100).await;
    progress
        .update("starting-mmproxy".to_string(), None, 0)
        .await;

    let base_node_grpc_port = state.node_manager.get_grpc_port().await?;

    let mut telemetry_id = state
        .telemetry_manager
        .read()
        .await
        .get_unique_string()
        .await;
    if telemetry_id.is_empty() {
        telemetry_id = "unknown_miner_tari_universe".to_string();
    }

    let config = state.config.read().await;
    let p2pool_port = state.p2pool_manager.grpc_port().await;
    mm_proxy_manager
        .start(StartConfig::new(
            state.shutdown.to_signal().clone(),
            app.path_resolver().app_local_data_dir().unwrap().clone(),
            app.path_resolver().app_config_dir().unwrap().clone(),
            app.path_resolver().app_log_dir().unwrap().clone(),
            cpu_miner_config.tari_address.clone(),
            base_node_grpc_port,
            telemetry_id,
            config.p2pool_enabled(),
            p2pool_port,
        ))
        .await?;
    mm_proxy_manager.wait_ready().await?;
    *state.is_setup_finished.write().await = true;
    drop(
        window
            .emit(
                "message",
                SetupStatusEvent {
                    event_type: "setup_status".to_string(),
                    title: "application-started".to_string(),
                    title_params: None,
                    progress: 1.0,
                },
            )
            .inspect_err(|e| error!(target: LOG_TARGET, "Could not emit event 'message': {:?}", e)),
    );

    Ok(())
}

#[tauri::command]
async fn set_p2pool_enabled(
    p2pool_enabled: bool,
    state: tauri::State<'_, UniverseAppState>,
) -> Result<(), String> {
    let timer = Instant::now();
    state
        .config
        .write()
        .await
        .set_p2pool_enabled(p2pool_enabled)
        .await
        .inspect_err(|e| error!("error at set_p2pool_enabled {:?}", e))
        .map_err(|e| e.to_string())?;

    let origin_config = state.mm_proxy_manager.config().await;
    let p2pool_grpc_port = state.p2pool_manager.grpc_port().await;
    if origin_config.is_none() {
        warn!(target: LOG_TARGET, "Tried to set p2pool_enabled but mmproxy has not been initialized yet");
        return Ok(());
    }
    let mut origin_config = origin_config.clone().unwrap();
    if origin_config.p2pool_enabled != p2pool_enabled {
        if p2pool_enabled {
            origin_config.set_to_use_p2pool(p2pool_grpc_port);
        } else {
            let base_node_grpc_port = state
                .node_manager
                .get_grpc_port()
                .await
                .map_err(|error| error.to_string())?;
            origin_config.set_to_use_base_node(base_node_grpc_port);
        };
        state
            .mm_proxy_manager
            .change_config(origin_config)
            .await
            .map_err(|error| error.to_string())?;
    }

    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET, "set_p2pool_enabled took too long: {:?}", timer.elapsed());
    }
    Ok(())
}

#[tauri::command]
async fn set_cpu_mining_enabled<'r>(
    enabled: bool,
    state: tauri::State<'_, UniverseAppState>,
) -> Result<(), String> {
    let timer = Instant::now();
    let mut config = state.config.write().await;
    config
        .set_cpu_mining_enabled(enabled)
        .await
        .inspect_err(|e| error!("error at set_cpu_mining_enabled {:?}", e))
        .map_err(|e| e.to_string())?;
    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET,
            "set_cpu_mining_enabled took too long: {:?}",
            timer.elapsed()
        );
    }
    Ok(())
}

#[tauri::command]
async fn set_gpu_mining_enabled(
    enabled: bool,
    state: tauri::State<'_, UniverseAppState>,
) -> Result<(), String> {
    let timer = Instant::now();
    let mut config = state.config.write().await;
    config
        .set_gpu_mining_enabled(enabled)
        .await
        .inspect_err(|e| error!("error at set_gpu_mining_enabled {:?}", e))
        .map_err(|e| e.to_string())?;
    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET,
            "set_gpu_mining_enabled took too long: {:?}",
            timer.elapsed()
        );
    }

    Ok(())
}

#[tauri::command]
async fn get_seed_words(
    _window: tauri::Window,
    _state: tauri::State<'_, UniverseAppState>,
    app: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    let timer = Instant::now();
    let config_path = app.path_resolver().app_config_dir().unwrap();
    let internal_wallet = InternalWallet::load_or_create(config_path)
        .await
        .map_err(|e| e.to_string())?;
    let seed_words = internal_wallet
        .decrypt_seed_words()
        .map_err(|e| e.to_string())?;
    let mut res = vec![];
    for i in 0..seed_words.len() {
        res.push(seed_words.get_word(i).unwrap().clone());
    }
    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET, "get_seed_words took too long: {:?}", timer.elapsed());
    }
    Ok(res)
}

#[tauri::command]
async fn start_mining<'r>(
    window: tauri::Window,
    state: tauri::State<'_, UniverseAppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let timer = Instant::now();
    let config = state.config.read().await;
    let cpu_mining_enabled = config.cpu_mining_enabled();
    let gpu_mining_enabled = config.gpu_mining_enabled();
    let mode = config.mode();

    let cpu_miner_config = state.cpu_miner_config.read().await;
    let monero_address = config.monero_address().to_string();
    let progress_tracker = ProgressTracker::new(window.clone());
    if cpu_mining_enabled {
        let mm_proxy_port = state
            .mm_proxy_manager
            .get_monero_port()
            .await
            .map_err(|e| e.to_string())?;

        let res = state
            .cpu_miner
            .write()
            .await
            .start(
                state.shutdown.to_signal(),
                &cpu_miner_config,
                monero_address.to_string(),
                mm_proxy_port,
                app.path_resolver().app_local_data_dir().unwrap(),
                app.path_resolver().app_cache_dir().unwrap(),
                app.path_resolver().app_config_dir().unwrap(),
                app.path_resolver().app_log_dir().unwrap(),
                progress_tracker,
                mode,
            )
            .await;

        if let Err(e) = res {
            error!(target: LOG_TARGET, "Could not start mining: {:?}", e);
            state
                .cpu_miner
                .write()
                .await
                .stop()
                .await
                .inspect_err(|e| error!("error at stopping cpu miner {:?}", e))
                .ok();
            return Err(e.to_string());
        }
    }

    let gpu_available = state.gpu_miner.read().await.is_gpu_mining_available();
    info!(target: LOG_TARGET, "Gpu availability {:?}", gpu_available.clone());

    if gpu_mining_enabled && gpu_available {
        let tari_address = state.cpu_miner_config.read().await.tari_address.clone();
        let p2pool_enabled = state.config.read().await.p2pool_enabled();
        let source = if p2pool_enabled {
            let p2pool_port = state.p2pool_manager.grpc_port().await;
            GpuNodeSource::P2Pool { port: p2pool_port }
        } else {
            let grpc_port = state
                .node_manager
                .get_grpc_port()
                .await
                .map_err(|e| e.to_string())?;

            GpuNodeSource::BaseNode { port: grpc_port }
        };

        let mut telemetry_id = state
            .telemetry_manager
            .read()
            .await
            .get_unique_string()
            .await;
        if telemetry_id.is_empty() {
            telemetry_id = "tari-universe".to_string();
        }

        let res = state
            .gpu_miner
            .write()
            .await
            .start(
                state.shutdown.to_signal(),
                tari_address,
                source,
                app.path_resolver().app_local_data_dir().unwrap(),
                app.path_resolver().app_config_dir().unwrap(),
                app.path_resolver().app_log_dir().unwrap(),
                mode,
                telemetry_id,
            )
            .await;

        if let Err(e) = res {
            error!(target: LOG_TARGET, "Could not start gpu mining: {:?}", e);
            drop(
                state.cpu_miner.write().await.stop().await.inspect_err(
                    |e| error!(target: LOG_TARGET, "Could not stop cpu miner: {:?}", e),
                ),
            );
            return Err(e.to_string());
        }
    }
    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET, "start_mining took too long: {:?}", timer.elapsed());
    }
    Ok(())
}

#[tauri::command]
async fn stop_mining<'r>(state: tauri::State<'_, UniverseAppState>) -> Result<(), String> {
    let timer = Instant::now();
    state
        .cpu_miner
        .write()
        .await
        .stop()
        .await
        .map_err(|e| e.to_string())?;

    state
        .gpu_miner
        .write()
        .await
        .stop()
        .await
        .map_err(|e| e.to_string())?;
    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET, "stop_mining took too long: {:?}", timer.elapsed());
    }
    Ok(())
}

#[tauri::command]
fn open_log_dir(app: tauri::AppHandle) {
    let log_dir = app.path_resolver().app_log_dir().unwrap();
    if let Err(e) = open::that(log_dir) {
        error!(target: LOG_TARGET, "Could not open log dir: {:?}", e);
    }
}

#[tauri::command]
async fn get_applications_versions(app: tauri::AppHandle) -> Result<ApplicationsVersions, String> {
    let timer = Instant::now();
    //TODO could be move to status command when XmrigAdapter will be implemented in BinaryResolver

    let default_message = "Failed to read version".to_string();

    let cache_dir = app.path_resolver().app_cache_dir().unwrap();

    let tari_universe_version = app.package_info().version.clone();
    let xmrig_version: String = XmrigAdapter::get_latest_local_version(cache_dir.clone())
        .await
        .unwrap_or(default_message);

    let minotari_node_version: semver::Version = BinaryResolver::current()
        .get_latest_version(Binaries::MinotariNode)
        .await;
    let mm_proxy_version: semver::Version = BinaryResolver::current()
        .get_latest_version(Binaries::MergeMiningProxy)
        .await;
    let wallet_version: semver::Version = BinaryResolver::current()
        .get_latest_version(Binaries::Wallet)
        .await;
    let sha_p2pool_version: semver::Version = BinaryResolver::current()
        .get_latest_version(Binaries::ShaP2pool)
        .await;

    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET,
            "get_applications_versions took too long: {:?}",
            timer.elapsed()
        );
    }

    Ok(ApplicationsVersions {
        tari_universe: tari_universe_version.to_string(),
        xmrig: xmrig_version,
        minotari_node: minotari_node_version.to_string(),
        mm_proxy: mm_proxy_version.to_string(),
        wallet: wallet_version.to_string(),
        sha_p2pool: sha_p2pool_version.to_string(),
    })
}

#[tauri::command]
async fn update_applications(
    app: tauri::AppHandle,
    state: tauri::State<'_, UniverseAppState>,
) -> Result<(), String> {
    let timer = Instant::now();
    state
        .config
        .write()
        .await
        .set_last_binaries_update_timestamp(SystemTime::now())
        .await
        .inspect_err(
            |e| error!(target: LOG_TARGET, "Could not set last binaries update timestamp: {:?}", e),
        )
        .map_err(|e| e.to_string())?;
    let progress_tracker = ProgressTracker::new(app.get_window("main").unwrap().clone());
    let cache_dir = app.path_resolver().app_cache_dir().unwrap();
    XmrigAdapter::ensure_latest(cache_dir, true, progress_tracker.clone())
        .await
        .map_err(|e| e.to_string())?;
    sleep(Duration::from_secs(1));
    BinaryResolver::current()
        .ensure_latest(Binaries::MinotariNode, progress_tracker.clone())
        .await
        .map_err(|e| e.to_string())?;
    sleep(Duration::from_secs(1));
    BinaryResolver::current()
        .ensure_latest(Binaries::MergeMiningProxy, progress_tracker.clone())
        .await
        .map_err(|e| e.to_string())?;
    sleep(Duration::from_secs(1));
    BinaryResolver::current()
        .ensure_latest(Binaries::Wallet, progress_tracker.clone())
        .await
        .map_err(|e| e.to_string())?;
    sleep(Duration::from_secs(1));
    BinaryResolver::current()
        .ensure_latest(Binaries::GpuMiner, progress_tracker.clone())
        .await
        .map_err(|e| e.to_string())?;
    sleep(Duration::from_secs(1));
    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET, "update_applications took too long: {:?}", timer.elapsed());
    }
    Ok(())
}

#[tauri::command]
async fn get_p2pool_stats(
    state: tauri::State<'_, UniverseAppState>,
) -> Result<HashMap<String, Stats>, String> {
    let timer = Instant::now();
    if state.is_getting_p2pool_stats.load(Ordering::SeqCst) {
        warn!(target: LOG_TARGET, "Already getting p2pool stats");
        return Err("Already getting p2pool stats".to_string());
    }
    state.is_getting_p2pool_stats.store(true, Ordering::SeqCst);
    let p2pool_stats = state.p2pool_manager.stats().await;

    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET, "get_p2pool_stats took too long: {:?}", timer.elapsed());
    }
    state.is_getting_p2pool_stats.store(false, Ordering::SeqCst);
    Ok(p2pool_stats)
}

#[tauri::command]
async fn get_tari_wallet_details(
    state: tauri::State<'_, UniverseAppState>,
) -> Result<TariWalletDetails, String> {
    let timer = Instant::now();
    if state.is_getting_wallet_balance.load(Ordering::SeqCst) {
        warn!(target: LOG_TARGET, "Already getting wallet balance");
        return Err("Already getting wallet balance".to_string());
    }
    state
        .is_getting_wallet_balance
        .store(true, Ordering::SeqCst);
    let wallet_balance = match state.wallet_manager.get_balance().await {
        Ok(w) => w,
        Err(e) => {
            if !matches!(e, WalletManagerError::WalletNotStarted) {
                warn!(target: LOG_TARGET, "Error getting wallet balance: {}", e);
            }

            WalletBalance {
                available_balance: MicroMinotari(0),
                pending_incoming_balance: MicroMinotari(0),
                pending_outgoing_balance: MicroMinotari(0),
                timelocked_balance: MicroMinotari(0),
            }
        }
    };
    let tari_address = state.tari_address.read().await;

    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET, "get_tari_wallet_details took too long: {:?}", timer.elapsed());
    }
    state
        .is_getting_wallet_balance
        .store(false, Ordering::SeqCst);
    Ok(TariWalletDetails {
        wallet_balance,
        tari_address_base58: tari_address.to_base58(),
        tari_address_emoji: tari_address.to_emoji_string(),
    })
}

#[tauri::command]
async fn get_app_config(
    _window: tauri::Window,
    state: tauri::State<'_, UniverseAppState>,
    _app: tauri::AppHandle,
) -> Result<AppConfig, String> {
    Ok(state.config.read().await.clone())
}

#[tauri::command]
async fn get_miner_metrics(
    state: tauri::State<'_, UniverseAppState>,
    app: tauri::AppHandle,
) -> Result<MinerMetrics, String> {
    let timer = Instant::now();
    if state.is_getting_miner_metrics.load(Ordering::SeqCst) {
        warn!(target: LOG_TARGET, "Already getting miner metrics");
        return Err("Already getting miner metrics".to_string());
    }
    state.is_getting_miner_metrics.store(true, Ordering::SeqCst);

    let mut cpu_miner = state.cpu_miner.write().await;
    let mut gpu_miner = state.gpu_miner.write().await;
    let (sha_hash_rate, randomx_hash_rate, block_reward, block_height, block_time, is_synced) =
        state
            .node_manager
            .get_network_hash_rate_and_block_reward()
            .await
            .unwrap_or_else(|e| {
                if !matches!(e, NodeManagerError::NodeNotStarted) {
                    warn!(target: LOG_TARGET, "Error getting network hash rate and block reward: {}", e);
                }
                (0, 0, MicroMinotari(0), 0, 0, false)
            });

    let cpu_mining_status = match cpu_miner
        .status(randomx_hash_rate, block_reward)
        .await
        .map_err(|e| e.to_string())
    {
        Ok(cpu) => cpu,
        Err(e) => {
            warn!(target: LOG_TARGET, "Error getting cpu miner status: {:?}", e);
            state
                .is_getting_miner_metrics
                .store(false, Ordering::SeqCst);
            return Err(e);
        }
    };

    let gpu_mining_status = match gpu_miner.status(sha_hash_rate, block_reward).await {
        Ok(gpu) => gpu,
        Err(e) => {
            warn!(target: LOG_TARGET, "Error getting gpu miner status: {:?}", e);
            state
                .is_getting_miner_metrics
                .store(false, Ordering::SeqCst);
            return Err(e.to_string());
        }
    };

    let hardware_status = HardwareMonitor::current()
        .write()
        .await
        .read_hardware_parameters();

    let new_systemtray_data: SystrayData = SystemtrayManager::current().create_systemtray_data(
        cpu_mining_status.hash_rate,
        gpu_mining_status.hash_rate as f64,
        hardware_status.clone(),
        cpu_mining_status.estimated_earnings as f64,
    );

    SystemtrayManager::current().update_systray(app, new_systemtray_data);

    let connected_peers = state
        .node_manager
        .list_connected_peers()
        .await
        .unwrap_or_default();

    if timer.elapsed() > MAX_ACCEPTABLE_COMMAND_TIME {
        warn!(target: LOG_TARGET, "get_miner_metrics took too long: {:?}", timer.elapsed());
    }
    state
        .is_getting_miner_metrics
        .store(false, Ordering::SeqCst);

    Ok(MinerMetrics {
        cpu: CpuMinerMetrics {
            hardware: hardware_status.cpu,
            mining: cpu_mining_status,
        },
        gpu: GpuMinerMetrics {
            hardware: hardware_status.gpu,
            mining: gpu_mining_status,
        },
        base_node: BaseNodeStatus {
            block_height,
            block_time,
            is_synced,
            is_connected: !connected_peers.is_empty(),
            connected_peers,
        },
    })
}

#[tauri::command]
fn log_web_message(level: String, message: Vec<String>) {
    match level.as_str() {
        "error" => error!(target: LOG_TARGET_WEB, "{}", message.join(" ")),
        _ => info!(target: LOG_TARGET_WEB, "{}", message.join(" ")),
    }
}

#[tauri::command]
async fn reset_settings<'r>(
    reset_wallet: bool,
    _window: tauri::Window,
    state: tauri::State<'_, UniverseAppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    stop_all_miners(state.inner().clone(), 5).await?;

    let app_config_dir = app.path_resolver().app_config_dir();
    let app_cache_dir = app.path_resolver().app_cache_dir();
    let app_data_dir = app.path_resolver().app_data_dir();
    let app_local_data_dir = app.path_resolver().app_local_data_dir();

    let dirs_to_remove = [
        app_config_dir,
        app_cache_dir,
        app_data_dir,
        app_local_data_dir,
    ];
    let missing_dirs: Vec<String> = dirs_to_remove
        .iter()
        .filter(|dir| dir.is_none())
        .map(|dir| dir.clone().unwrap().to_str().unwrap().to_string())
        .collect();

    if !missing_dirs.is_empty() {
        error!(target: LOG_TARGET, "Could not get app directories for {:?}", missing_dirs);
        return Err("Could not get app directories".to_string());
    }

    // Exclude EBWebView because it is still being used.
    let mut folder_block_list = vec!["EBWebView"];
    if !reset_wallet {
        folder_block_list.push("esmeralda");
        folder_block_list.push("nextnet");
    }

    for dir in &dirs_to_remove {
        // check if dir exists
        if dir.clone().unwrap().exists() {
            for entry in read_dir(dir.clone().unwrap()).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                if path.is_dir() {
                    if folder_block_list.contains(&path.file_name().unwrap().to_str().unwrap()) {
                        continue;
                    }
                    debug!(target: LOG_TARGET, "[reset_settings] Removing {:?} directory", path);
                    remove_dir_all(path.clone()).map_err(|e| {
                        error!(target: LOG_TARGET, "[reset_settings] Could not remove {:?} directory: {:?}", path, e);
                        format!("Could not remove directory: {}", e)
                    })?;
                } else {
                    debug!(target: LOG_TARGET, "[reset_settings] Removing {:?} file", path);
                    remove_file(path.clone()).map_err(|e| {
                        error!(target: LOG_TARGET, "[reset_settings] Could not remove {:?} file: {:?}", path, e);
                        format!("Could not remove file: {}", e)
                    })?;
                }
            }
        }
    }

    info!(target: LOG_TARGET, "[reset_settings] Restarting the app");
    app.restart();

    Ok(())
}

#[derive(Debug, Serialize)]
pub struct CpuMinerMetrics {
    hardware: Option<HardwareParameters>,
    mining: CpuMinerStatus,
}

#[derive(Debug, Serialize)]
pub struct GpuMinerMetrics {
    hardware: Option<HardwareParameters>,
    mining: GpuMinerStatus,
}

#[derive(Debug, Serialize)]
pub struct MinerMetrics {
    cpu: CpuMinerMetrics,
    gpu: GpuMinerMetrics,
    base_node: BaseNodeStatus,
}

#[derive(Debug, Serialize)]
pub struct TariWalletDetails {
    wallet_balance: WalletBalance,
    tari_address_base58: String,
    tari_address_emoji: String,
}

#[derive(Debug, Serialize)]
pub struct ApplicationsVersions {
    tari_universe: String,
    xmrig: String,
    minotari_node: String,
    mm_proxy: String,
    wallet: String,
    sha_p2pool: String,
}

#[derive(Debug, Serialize)]
pub struct BaseNodeStatus {
    block_height: u64,
    block_time: u64,
    is_synced: bool,
    is_connected: bool,
    connected_peers: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct CpuMinerStatus {
    pub is_mining: bool,
    pub hash_rate: f64,
    pub estimated_earnings: u64,
    pub connection: CpuMinerConnectionStatus,
}

#[derive(Debug, Serialize)]
pub struct CpuMinerConnectionStatus {
    pub is_connected: bool,
    // pub error: Option<String>,
}

pub enum CpuMinerConnection {
    BuiltInProxy,
}

struct CpuMinerConfig {
    node_connection: CpuMinerConnection,
    tari_address: TariAddress,
}

#[derive(Clone)]
struct UniverseAppState {
    is_getting_wallet_balance: Arc<AtomicBool>,
    is_getting_p2pool_stats: Arc<AtomicBool>,
    is_getting_miner_metrics: Arc<AtomicBool>,
    is_setup_finished: Arc<RwLock<bool>>,
    config: Arc<RwLock<AppConfig>>,
    in_memory_config: Arc<RwLock<AppInMemoryConfig>>,
    shutdown: Shutdown,
    tari_address: Arc<RwLock<TariAddress>>,
    cpu_miner: Arc<RwLock<CpuMiner>>,
    gpu_miner: Arc<RwLock<GpuMiner>>,
    cpu_miner_config: Arc<RwLock<CpuMinerConfig>>,
    mm_proxy_manager: MmProxyManager,
    node_manager: NodeManager,
    wallet_manager: WalletManager,
    telemetry_manager: Arc<RwLock<TelemetryManager>>,
    feedback: Arc<RwLock<Feedback>>,
    airdrop_access_token: Arc<RwLock<Option<String>>>,
    p2pool_manager: P2poolManager,
}

#[derive(Clone, serde::Serialize)]
struct Payload {
    args: Vec<String>,
    cwd: String,
}

#[allow(clippy::too_many_lines)]
fn main() {
    let mut shutdown = Shutdown::new();

    // NOTE: Nothing is started at this point, so ports are not known. You can only start settings ports
    // and addresses once the different services have been started.
    // A better way is to only provide the config when we start the service.
    let node_manager = NodeManager::new();
    let wallet_manager = WalletManager::new(node_manager.clone());
    let wallet_manager2 = wallet_manager.clone();
    let p2pool_manager = P2poolManager::new();

    let cpu_config = Arc::new(RwLock::new(CpuMinerConfig {
        node_connection: CpuMinerConnection::BuiltInProxy,
        tari_address: TariAddress::default(),
    }));

    let app_in_memory_config = if cfg!(feature = "airdrop-local") {
        Arc::new(RwLock::new(
            app_in_memory_config::AppInMemoryConfig::init_local(),
        ))
    } else {
        Arc::new(RwLock::new(app_in_memory_config::AppInMemoryConfig::init()))
    };

    let cpu_miner: Arc<RwLock<CpuMiner>> = Arc::new(CpuMiner::new().into());
    let gpu_miner: Arc<RwLock<GpuMiner>> = Arc::new(GpuMiner::new().into());

    let app_config_raw = AppConfig::new();
    let app_config = Arc::new(RwLock::new(app_config_raw.clone()));

    let telemetry_manager: TelemetryManager = TelemetryManager::new(
        node_manager.clone(),
        cpu_miner.clone(),
        gpu_miner.clone(),
        app_config.clone(),
        app_in_memory_config.clone(),
        Some(Network::default()),
    );

    let feedback = Feedback::new(app_in_memory_config.clone(), app_config.clone());
    // let mm_proxy_config = if app_config_raw.p2pool_enabled {
    //     MergeMiningProxyConfig::new_with_p2pool(mm_proxy_port, p2pool_config.grpc_port, None)
    // } else {
    //     MergeMiningProxyConfig::new(
    //         mm_proxy_port,
    //         p2pool_config.grpc_port,
    //         base_node_grpc_port,
    //         None,
    //     )
    // };
    let mm_proxy_manager = MmProxyManager::new();
    let app_state = UniverseAppState {
        is_getting_miner_metrics: Arc::new(AtomicBool::new(false)),
        is_getting_p2pool_stats: Arc::new(AtomicBool::new(false)),
        is_getting_wallet_balance: Arc::new(AtomicBool::new(false)),
        is_setup_finished: Arc::new(RwLock::new(false)),
        config: app_config.clone(),
        in_memory_config: app_in_memory_config.clone(),
        shutdown: shutdown.clone(),
        tari_address: Arc::new(RwLock::new(TariAddress::default())),
        cpu_miner: cpu_miner.clone(),
        gpu_miner: gpu_miner.clone(),
        cpu_miner_config: cpu_config.clone(),
        mm_proxy_manager: mm_proxy_manager.clone(),
        node_manager,
        wallet_manager,
        p2pool_manager,
        telemetry_manager: Arc::new(RwLock::new(telemetry_manager)),
        feedback: Arc::new(RwLock::new(feedback)),
        airdrop_access_token: Arc::new(RwLock::new(None)),
    };

    let systray = SystemtrayManager::current().get_systray().clone();

    let app = tauri::Builder::default()
        .system_tray(systray)
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            println!("{}, {argv:?}, {cwd}", app.package_info().name);

            app.emit_all("single-instance", Payload { args: argv, cwd })
                .unwrap();
        }))
        .manage(app_state.clone())
        .setup(|app| {
            tari_common::initialize_logging(
                &app.path_resolver()
                    .app_config_dir()
                    .unwrap()
                    .join("log4rs_config.yml"),
                &app.path_resolver().app_log_dir().unwrap(),
                include_str!("../log4rs_sample.yml"),
            )
            .expect("Could not set up logging");

            let config_path = app.path_resolver().app_config_dir().unwrap();
            let thread_config = tauri::async_runtime::spawn(async move {
                app_config.write().await.load_or_create(config_path).await
            });

            match tauri::async_runtime::block_on(thread_config).unwrap() {
                Ok(_) => {}
                Err(e) => {
                    error!(target: LOG_TARGET, "Error setting up app state: {:?}", e);
                }
            };

            let config_path = app.path_resolver().app_config_dir().unwrap();
            let address = app.state::<UniverseAppState>().tari_address.clone();
            let thread = tauri::async_runtime::spawn(async move {
                match InternalWallet::load_or_create(config_path).await {
                    Ok(wallet) => {
                        cpu_config.write().await.tari_address = wallet.get_tari_address();
                        wallet_manager2
                            .set_view_private_key_and_spend_key(
                                wallet.get_view_key(),
                                wallet.get_spend_key(),
                            )
                            .await;
                        let mut lock = address.write().await;
                        *lock = wallet.get_tari_address();
                        Ok(())
                        //app.state::<UniverseAppState>().tari_address = wallet.get_tari_address();
                    }
                    Err(e) => {
                        error!(target: LOG_TARGET, "Error loading internal wallet: {:?}", e);
                        // TODO: If this errors, the application does not exit properly.
                        // So temporarily we are going to kill it here

                        Err(e)
                    }
                }
            });

            match tauri::async_runtime::block_on(thread).unwrap() {
                Ok(_) => {
                    // let mut lock = app.state::<UniverseAppState>().tari_address.write().await;
                    // *lock = address;
                    Ok(())
                }
                Err(e) => {
                    error!(target: LOG_TARGET, "Error setting up internal wallet: {:?}", e);
                    Err(e.into())
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            setup_application,
            start_mining,
            stop_mining,
            set_p2pool_enabled,
            set_mode,
            open_log_dir,
            get_seed_words,
            get_applications_versions,
            send_feedback,
            update_applications,
            log_web_message,
            set_allow_telemetry,
            set_airdrop_access_token,
            get_app_id,
            get_app_in_memory_config,
            set_monero_address,
            update_applications,
            reset_settings,
            set_gpu_mining_enabled,
            set_cpu_mining_enabled,
            restart_application,
            get_miner_metrics,
            get_app_config,
            get_p2pool_stats,
            get_tari_wallet_details,
            exit_application
        ])
        .build(tauri::generate_context!())
        .inspect_err(
            |e| error!(target: LOG_TARGET, "Error while building tauri application: {:?}", e),
        )
        .expect("error while running tauri application");

    info!(
        target: LOG_TARGET,
        "Starting Tari Universe version: {}",
        app.package_info().version
    );

    println!(
        "Logs stored at {:?}",
        app.path_resolver().app_log_dir().unwrap()
    );

    let mut downloaded: u64 = 0;
    app.run(move |_app_handle, event| match event {
        tauri::RunEvent::Updater(updater_event) => match updater_event {
            UpdaterEvent::Error(e) => {
                error!(target: LOG_TARGET, "Updater error: {:?}", e);
            }
            UpdaterEvent::DownloadProgress { chunk_length, content_length } => {
                downloaded += chunk_length as u64;
                let window = _app_handle.get_window("main").unwrap();
                drop(window.emit("update-progress", UpdateProgressRustEvent {chunk_length, content_length, downloaded}));
            }
            UpdaterEvent::Downloaded => {
                shutdown.trigger();
            }
            _ => {
                info!(target: LOG_TARGET, "Updater event: {:?}", updater_event);
            }
        },
        tauri::RunEvent::ExitRequested { api: _, .. } => {
            // api.prevent_exit();
            info!(target: LOG_TARGET, "App shutdown caught");
            let _unused = block_on(stop_all_miners(app_state.clone(), 2));
            info!(target: LOG_TARGET, "App shutdown complete");
        }
        tauri::RunEvent::Exit => {
            info!(target: LOG_TARGET, "App shutdown caught");
            let _unused = block_on(stop_all_miners(app_state.clone(), 2));
            info!(target: LOG_TARGET, "Tari Universe v{} shut down successfully", _app_handle.package_info().version);
        }
        RunEvent::MainEventsCleared => {
            // no need to handle
        }
        RunEvent::WindowEvent { label, event, .. }  => {
            trace!(target: LOG_TARGET, "Window event: {:?} {:?}", label, event);
        }
        _ => {
            debug!(target: LOG_TARGET, "Unhandled event: {:?}", event);
        }
    });
}
