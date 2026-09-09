use crate::config::{
    DeployServer, PostInstallActions, PostInstallTopologyMode, PostInstallTopologySettings,
};
use crate::deployment_topology::{
    DeploymentTopologyRequest, DeploymentTopologyResult, TopologyMode, TopologyResultStatus,
};
use crate::task_domain::DeployStage;
use crate::task_manager::DeployTrackingContext;
use crate::ums_init_password::{UmsInitPasswordRequest, UmsInitPasswordTargets};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct PostInstallError {
    pub stage: DeployStage,
    pub message: String,
}

fn stage_error(stage: DeployStage, message: impl Into<String>) -> PostInstallError {
    PostInstallError {
        stage,
        message: message.into(),
    }
}

fn cancelled(should_cancel: &AtomicBool) -> Result<(), String> {
    if should_cancel.load(Ordering::SeqCst) {
        Err("Deployment cancelled".to_string())
    } else {
        Ok(())
    }
}

fn interruptible_sleep(duration: Duration, should_cancel: &AtomicBool) -> Result<(), String> {
    let step = Duration::from_millis(250);
    let mut remaining = duration;
    while !remaining.is_zero() {
        cancelled(should_cancel)?;
        let current = remaining.min(step);
        std::thread::sleep(current);
        remaining = remaining.saturating_sub(current);
    }
    Ok(())
}

fn probe_service(host: &str, port: u16, timeout: Duration) -> Result<(), String> {
    let address: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|error| format!("无效地址 {host}:{port}: {error}"))?;
    TcpStream::connect_timeout(&address, timeout)
        .map(|_| ())
        .map_err(|error| format!("{host}:{port} 尚未就绪: {error}"))
}

fn wait_until_restarted(
    server: &DeployServer,
    actions: &PostInstallActions,
    should_cancel: &AtomicBool,
    tracking: Option<&DeployTrackingContext>,
) -> Result<(), String> {
    let interval = Duration::from_secs(actions.poll_interval_secs.max(1));
    let timeout = Duration::from_secs(server.ssh_timeout_secs.clamp(1, 10));
    let attempts = actions.poll_attempts.max(1);
    let mut last_error = "设备尚未就绪".to_string();

    // Give the appliance time to enter its reboot cycle so an old, still-live
    // session cannot be mistaken for the restarted service.
    interruptible_sleep(interval, should_cancel)?;

    for attempt in 0..attempts {
        cancelled(should_cancel)?;
        if let Some(tracking) = tracking {
            let next_poll_at = (chrono::Local::now()
                + chrono::Duration::seconds(actions.poll_interval_secs.max(1) as i64))
            .to_rfc3339();
            let _ =
                tracking.mark_checkpoint(&server.id, attempt + 1, Some(next_poll_at), true, false);
        }
        let probe = if actions.enable_ssh {
            crate::ssh_connection::resolve_and_connect(server).map(|_| ())
        } else {
            let password_targets = &actions.passwords;
            let port = if password_targets.framework {
                21_900
            } else if password_targets.ums {
                80
            } else {
                25_011
            };
            probe_service(&server.host, port, timeout)
        };
        match probe {
            Ok(()) => {
                if let Some(tracking) = tracking {
                    let _ = tracking.mark_checkpoint(&server.id, attempt + 1, None, false, false);
                }
                return Ok(());
            }
            Err(error) => last_error = error,
        }
        if attempt + 1 < attempts {
            interruptible_sleep(interval, should_cancel)?;
        }
    }

    Err(format!(
        "等待一体机重启完成超时（{} 次，每次间隔 {} 秒）：{}",
        attempts,
        actions.poll_interval_secs.max(1),
        last_error
    ))
}

fn mark_stage(tracking: Option<&DeployTrackingContext>, server: &DeployServer, stage: DeployStage) {
    if let Some(tracking) = tracking {
        let _ = tracking.mark_stage(&server.id, stage, Some(100.0), None);
    }
}

pub(crate) fn run_server_post_install<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    server: &DeployServer,
    actions: &PostInstallActions,
    should_cancel: &AtomicBool,
    tracking: Option<&DeployTrackingContext>,
    api_timeout_secs: u64,
    include_passwords: bool,
) -> Result<(), PostInstallError> {
    if !actions.enabled
        || (!actions.enable_ssh && !actions.passwords.enabled && !actions.topology.enabled)
    {
        return Ok(());
    }

    mark_stage(tracking, server, DeployStage::WaitingReboot);
    if let Some(tracking) = tracking {
        let _ = tracking.record_log(
            Some(&server.id),
            Some(&server.name),
            "info",
            "安装命令已完成，开始等待一体机重启并恢复服务",
        );
    }
    wait_until_restarted(server, actions, should_cancel, tracking)
        .map_err(|message| stage_error(DeployStage::WaitingReboot, message))?;

    if actions.enable_ssh {
        mark_stage(tracking, server, DeployStage::EnablingSsh);
        let summary = crate::ssh_connection::enable_ssh_and_all_tcp(server)
            .map_err(|message| stage_error(DeployStage::EnablingSsh, message))?;
        if let Some(tracking) = tracking {
            let _ = tracking.record_log(
                Some(&server.id),
                Some(&server.name),
                "success",
                &format!(
                    "SSH 已开启并放行全部来源的全部 TCP 端口，连接入口为 {}@{}:{}",
                    summary.username, summary.host, summary.port
                ),
            );
        }
    }

    if include_passwords && actions.passwords.enabled {
        run_password_post_install(app_handle, server, actions, tracking, api_timeout_secs)?;
    }

    Ok(())
}

pub(crate) fn run_server_post_install_from_stage<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    server: &DeployServer,
    actions: &PostInstallActions,
    start_stage: &DeployStage,
    should_cancel: &AtomicBool,
    tracking: Option<&DeployTrackingContext>,
    api_timeout_secs: u64,
    include_passwords: bool,
) -> Result<(), PostInstallError> {
    match start_stage {
        DeployStage::WaitingReboot => run_server_post_install(
            app_handle,
            server,
            actions,
            should_cancel,
            tracking,
            api_timeout_secs,
            include_passwords,
        ),
        DeployStage::EnablingSsh => {
            cancelled(should_cancel)
                .map_err(|message| stage_error(DeployStage::EnablingSsh, message))?;
            if actions.enable_ssh {
                mark_stage(tracking, server, DeployStage::EnablingSsh);
                crate::ssh_connection::enable_ssh_and_all_tcp(server)
                    .map_err(|message| stage_error(DeployStage::EnablingSsh, message))?;
            }
            if include_passwords && actions.passwords.enabled {
                run_password_post_install(app_handle, server, actions, tracking, api_timeout_secs)?;
            }
            Ok(())
        }
        DeployStage::ChangingPasswords => {
            run_password_post_install(app_handle, server, actions, tracking, api_timeout_secs)
        }
        _ => Ok(()),
    }
}

pub(crate) fn run_password_post_install<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    server: &DeployServer,
    actions: &PostInstallActions,
    tracking: Option<&DeployTrackingContext>,
    api_timeout_secs: u64,
) -> Result<(), PostInstallError> {
    if actions.enabled && actions.passwords.enabled {
        let passwords = &actions.passwords;
        if !passwords.framework && !passwords.ums && !passwords.cdm {
            return Err(stage_error(
                DeployStage::ChangingPasswords,
                "已启用安装后改密，但未选择框架、UMS 或 CDM",
            ));
        }
        mark_stage(tracking, server, DeployStage::ChangingPasswords);
        let request = UmsInitPasswordRequest {
            ips: vec![server.host.clone()],
            targets: UmsInitPasswordTargets {
                framework: passwords.framework,
                ums: passwords.ums,
                cdm: passwords.cdm,
            },
            ums_username: passwords.ums_username.clone(),
            framework_new_password: passwords.framework_new_password.clone(),
            ums_new_password: passwords.ums_new_password.clone(),
            cdm_new_password: passwords.cdm_new_password.clone(),
            framework_old_password: passwords.framework_old_password.clone(),
            ums_old_password: passwords.ums_old_password.clone(),
            cdm_old_password: passwords.cdm_old_password.clone(),
        };
        let results =
            tauri::async_runtime::block_on(crate::ums_init_password::execute_ums_init_password(
                request,
                app_handle.clone(),
                api_timeout_secs.max(1),
            ))
            .map_err(|message| stage_error(DeployStage::ChangingPasswords, message))?;
        let failures = results
            .iter()
            .flat_map(|result| result.targets.iter())
            .filter(|target| !target.success)
            .map(|target| target.message.clone())
            .collect::<Vec<_>>();
        if !failures.is_empty() {
            return Err(stage_error(
                DeployStage::ChangingPasswords,
                format!("安装后初始密码修改失败：{}", failures.join("；")),
            ));
        }
    }

    Ok(())
}

fn topology_mode(mode: &PostInstallTopologyMode) -> TopologyMode {
    match mode {
        PostInstallTopologyMode::Ha => TopologyMode::Ha,
        PostInstallTopologyMode::Replica => TopologyMode::Replica,
        PostInstallTopologyMode::HaAndReplica => TopologyMode::HaAndReplica,
    }
}

pub(crate) fn topology_request(
    settings: &PostInstallTopologySettings,
    poll_interval_secs: u64,
    poll_attempts: u32,
) -> DeploymentTopologyRequest {
    DeploymentTopologyRequest {
        mode: topology_mode(&settings.mode),
        primary_ip: settings.primary_ip.trim().to_string(),
        ha_replica_ip: (!settings.ha_replica_ip.trim().is_empty())
            .then(|| settings.ha_replica_ip.trim().to_string()),
        virtual_ip: (!settings.virtual_ip.trim().is_empty())
            .then(|| settings.virtual_ip.trim().to_string()),
        replica_ips: settings
            .replica_ips
            .iter()
            .map(|ip| ip.trim().to_string())
            .filter(|ip| !ip.is_empty())
            .collect(),
        framework_password: settings.framework_password.clone(),
        poll_interval_secs: poll_interval_secs.max(1),
        poll_attempts: poll_attempts.max(1),
    }
}

pub(crate) fn run_topology_post_install(
    settings: &PostInstallTopologySettings,
    poll_interval_secs: u64,
    poll_attempts: u32,
) -> Result<DeploymentTopologyResult, String> {
    let request = topology_request(settings, poll_interval_secs, poll_attempts);
    tauri::async_runtime::block_on(crate::deployment_topology::execute_deployment_topology(
        request,
    ))
}

pub(crate) fn query_topology_post_install(
    settings: &PostInstallTopologySettings,
    poll_interval_secs: u64,
    poll_attempts: u32,
) -> Result<DeploymentTopologyResult, String> {
    let request = topology_request(settings, poll_interval_secs, poll_attempts);
    tauri::async_runtime::block_on(crate::deployment_topology::query_deployment_topology(
        request,
    ))
}

pub(crate) fn topology_stage(result: &DeploymentTopologyResult) -> DeployStage {
    match result.status {
        TopologyResultStatus::Unconfirmed => DeployStage::Unconfirmed,
        _ => DeployStage::VerifyingTopology,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topology_mode_mapping_is_exact() {
        assert_eq!(
            topology_mode(&PostInstallTopologyMode::Ha),
            TopologyMode::Ha
        );
        assert_eq!(
            topology_mode(&PostInstallTopologyMode::Replica),
            TopologyMode::Replica
        );
        assert_eq!(
            topology_mode(&PostInstallTopologyMode::HaAndReplica),
            TopologyMode::HaAndReplica
        );
    }
}
