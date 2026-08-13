use crate::backup::run_backup;
use crate::config::{
    ensure_agent_defaults, resolve_hostname, AppConfig, Destination, LocalDestination,
};
use crate::presets::load_merged_presets;
use std::fs;

#[test]
fn presets_parse_and_backup_local() {
    let presets = load_merged_presets().expect("presets");
    assert!(presets.agents.iter().any(|a| a.key == "cursor"));

    let mut cfg = AppConfig::default();
    ensure_agent_defaults(&mut cfg, &presets);
    cfg.encryption.enabled = false;

    let dest = std::env::temp_dir().join("agent-backup-smoke-dest");
    let _ = fs::remove_dir_all(&dest);
    fs::create_dir_all(&dest).unwrap();
    cfg.destinations.push(Destination::Local {
        id: "dest_test".into(),
        name: "tmp".into(),
        enabled: true,
        local: LocalDestination {
            root_path: dest.to_string_lossy().into(),
        },
    });

    let result = run_backup(&cfg, "manual").expect("backup");
    assert!(
        result.overall_status == "ok" || result.overall_status == "partial",
        "{}",
        result.message
    );

    let host = resolve_hostname(&cfg);
    let dir = dest.join("backups").join(host).join(&result.created_at);
    assert!(
        dir.join("manifest.json").exists(),
        "missing manifest in {:?}",
        dir
    );
}
