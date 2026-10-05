//! Explicit local diagnosis entry; uses the same injected Core/Adapters and only writes normalized DTOs.
use crate::{composition, mapping};
use std::path::Path;
use usage_contracts as api;
use usage_core::{CancellationToken, CollectRequest};

pub fn run(output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?;
    let scratch = tempfile::tempdir()?;
    // Separate temporary identities/repository: diagnosis never edits the running application's settings/cache.
    let (service, configs, _) = composition::bootstrap(
        scratch.path(),
        executable.parent().ok_or("missing executable directory")?,
    )?;
    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(async {
        let mut statuses = vec![];
        for provider in ["ccusage.codex", "ccusage.antigravity"] {
            let mut config = configs.get(provider).cloned().unwrap_or_default(); config.enabled = true;
            let scan = service.run_scan(uuid::Uuid::new_v4().to_string(), provider.into(), CollectRequest { timezone: "UTC".into(), config }, CancellationToken::default()).await?;
            statuses.push(mapping::scan(scan));
        }
        let snapshots = service.repository.load_snapshots(Default::default()).await?;
        let earliest = snapshots.iter().filter_map(|snapshot| snapshot.coverage.observed_from).min().unwrap_or_else(|| chrono::Utc::now().date_naive());
        let latest = snapshots.iter().filter_map(|snapshot| snapshot.coverage.observed_until).max().unwrap_or_else(|| earliest.succ_opt().unwrap());
        let query = api::OverviewQuery { range: api::DateRange { start: earliest.to_string(), end: latest.to_string() }, timezone:"UTC".into(), provider_ids: vec!["ccusage.codex".into(), "ccusage.antigravity".into()], model_ids:vec![], bucket:api::Bucket::Month };
        let overview = mapping::overview(service.get_overview(mapping::overview_query(&query)?).await?, query);
        Ok::<_, usage_core::CoreError>(serde_json::json!({ "apiVersion":api::API_VERSION, "appVersion":env!("CARGO_PKG_VERSION"), "scans":statuses, "overview":overview }))
    })?;
    std::fs::write(output, serde_json::to_vec_pretty(&result)?)?;
    Ok(())
}
