use rustmotion::engine::preload::scenario_icon_names;
use rustmotion::engine::renderer::{
    icon_cache_dir, icon_source_cache_file, set_remote_icon_policy, RemoteIconPolicy,
};
use rustmotion::error::Result;
use rustmotion::loader::load_scenario_from_source;
use std::path::{Path, PathBuf};

fn scenario_at(file: &Path) -> Result<rustmotion::schema::ResolvedScenario> {
    load_scenario_from_source(Some(&PathBuf::from(file)), None)
}

fn icons_named_by(file: &Path) -> Result<Vec<String>> {
    Ok(scenario_icon_names(&scenario_at(file)?))
}

fn split_by_cache_presence(icons: &[String]) -> (Vec<String>, Vec<String>) {
    let cache_dir = icon_cache_dir();
    icons.iter().cloned().partition(|icon| {
        icon_source_cache_file(&cache_dir, icon)
            .metadata()
            .is_ok_and(|m| m.len() > 0)
    })
}

pub fn cmd_icons_check(file: &Path, quiet: bool) -> Result<()> {
    let icons = icons_named_by(file)?;
    let (cached, missing) = split_by_cache_presence(&icons);

    if quiet {
        for icon in &missing {
            println!("{icon}");
        }
        return Ok(());
    }

    println!("Icon cache: {}", icon_cache_dir().display());
    println!(
        "{} icon(s) named, {} already cached",
        icons.len(),
        cached.len()
    );

    if missing.is_empty() {
        println!("Nothing to fetch — this scenario renders offline.");
        return Ok(());
    }

    println!("\n{} missing:", missing.len());
    for icon in &missing {
        println!("  {icon}");
    }
    println!(
        "\nRun `rustmotion icons prefetch -f {}` to download them.",
        file.display()
    );
    Ok(())
}

pub fn cmd_icons_prefetch(file: &Path, quiet: bool) -> Result<()> {
    let icons = icons_named_by(file)?;
    let (_, missing) = split_by_cache_presence(&icons);

    if missing.is_empty() {
        if !quiet {
            println!(
                "All {} icon(s) are already in {}.",
                icons.len(),
                icon_cache_dir().display()
            );
        }
        return Ok(());
    }

    set_remote_icon_policy(RemoteIconPolicy::Allow);

    let scenario = scenario_at(file)?;
    for view in &scenario.views {
        rustmotion::engine::preload::prefetch_icons(&view.scenes)?;
    }

    let (cached_after, still_missing) = split_by_cache_presence(&icons);
    if !quiet {
        println!(
            "Fetched {} icon(s) into {}. {} of {} now cached.",
            missing.len() - still_missing.len(),
            icon_cache_dir().display(),
            cached_after.len(),
            icons.len()
        );
    }
    Ok(())
}
