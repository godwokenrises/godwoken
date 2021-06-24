use crate::deploy_genesis::deploy_genesis;
use crate::deploy_scripts::deploy_scripts;
use crate::generate_config::generate_config;
use crate::prepare_scripts::{self, prepare_scripts, ScriptsBuildMode};
use crate::setup_nodes::{self, setup_nodes};
use crate::utils;
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub fn setup(
    ckb_rpc_url: &str,
    indexer_url: &str,
    mode: ScriptsBuildMode,
    scripts_path: &Path,
    privkey_path: &Path,
    nodes_count: u8,
    server_url: &str,
    output_dir: &Path,
) {
    let prepare_scripts_result = utils::make_path(output_dir, vec!["scripts-deploy.json"]);
    prepare_scripts(
        mode,
        scripts_path,
        Path::new(prepare_scripts::REPOS_DIR_PATH),
        Path::new(prepare_scripts::SCRIPTS_DIR_PATH),
        &prepare_scripts_result,
    )
    .expect("prepare scripts");

    let scripts_deployment_result =
        utils::make_path(output_dir, vec!["scripts-deploy-result.json"]);
    deploy_scripts(
        privkey_path,
        ckb_rpc_url,
        &prepare_scripts_result,
        &scripts_deployment_result,
    )
    .expect("deploy scripts");

    let poa_config_path = utils::make_path(output_dir, vec!["poa-config.json"]);
    let rollup_config_path = utils::make_path(output_dir, vec!["rollup-config.json"]);
    let capacity = setup_nodes::TRANSFER_CAPACITY
        .parse()
        .expect("get capacity");
    setup_nodes(
        privkey_path,
        capacity,
        nodes_count,
        output_dir,
        &poa_config_path,
        &rollup_config_path,
    );

    let genesis_deploy_result = utils::make_path(output_dir, vec!["genesis-deploy-result.json"]);
    deploy_genesis(
        privkey_path,
        ckb_rpc_url,
        &scripts_deployment_result,
        &rollup_config_path,
        &poa_config_path,
        None,
        &genesis_deploy_result,
    )
    .expect("deploy genesis");

    (0..nodes_count).for_each(|index| {
        let node_name = format!("node{}", index + 1);
        let privkey_path = utils::make_path(output_dir, vec![&node_name, &"pk".to_owned()]);
        let output_file_path =
            utils::make_path(output_dir, vec![node_name, "config.toml".to_owned()]);
        generate_config(
            &genesis_deploy_result,
            &scripts_deployment_result,
            privkey_path.as_ref(),
            ckb_rpc_url.to_owned(),
            indexer_url.to_owned(),
            output_file_path.as_ref(),
            None,
            &prepare_scripts_result,
            server_url.to_string(),
        )
        .expect("generate_config");
    });

    log::info!("Finish");
}
