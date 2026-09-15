//! Diagnostic: print what this shell would see on the machine, and what the
//! update dialog would offer.
//!
//! ```text
//! cargo run --example check-updates        # detect everything
//! cargo run --example check-updates 0.1.5-rc.1   # pretend this is installed
//! ```

use dsh_xswt_tauriapp_core::{server, updates};

fn main() {
    println!("== 环境 ==");
    println!("DSH_HOME       : {}", server::dsh_home().display());
    match server::resolve_node() {
        Some(node) => println!("node           : {}", node.display()),
        None => println!("node           : ✗ 未找到"),
    }
    match server::resolve_dsh_bin() {
        Some(bin) => println!("dsh launcher   : {}", bin.display()),
        None => println!("dsh launcher   : ✗ 未找到"),
    }
    match server::installed_version() {
        Some(version) => println!("dsh 版本       : {version}"),
        None => println!("dsh 版本       : ✗ 无法读取"),
    }
    println!("日志目录       : {}", server::log_dir().display());

    println!(
        "\n== 端口扫描 ({}–{}) ==",
        server::START_PORT,
        server::MAX_PORT
    );
    match server::find_running_url() {
        Some((port, url)) => println!("已发现运行中的服务: :{port} → {url}"),
        None => println!("未发现运行中的 dsh 服务"),
    }
    match server::find_free_port() {
        Some(port) => println!("首个空闲端口   : {port}"),
        None => println!("✗ 端口段全被占用"),
    }

    let current = std::env::args()
        .nth(1)
        .or_else(server::installed_version)
        .unwrap_or_else(|| "0.0.0".to_string());

    println!("\n== 检查更新 (当前 {current}) ==");
    let store = updates::DismissStore::in_memory();
    match updates::check(&current, &store) {
        Err(error) => println!("✗ {error}"),
        Ok(report) => {
            for channel in &report.channels {
                let tags = if channel.dist_tags.is_empty() {
                    String::new()
                } else {
                    format!("  [tag: {}]", channel.dist_tags.join(", "))
                };
                let mark = if channel.newer { " ↑可更新" } else { "" };
                println!("\n-- {} ({}){tags}{mark}", channel.label, channel.id);
                match &channel.latest {
                    None => println!("   (该通道暂无版本)"),
                    Some(latest) => {
                        println!(
                            "   latest: {}  ({})",
                            latest.version,
                            latest.published.as_deref().unwrap_or("时间未知")
                        );
                        for entry in channel.versions.iter().skip(1) {
                            println!("           {}", entry.version);
                        }
                    }
                }
            }
            println!("\n== 结论 ==");
            match &report.candidate {
                None => println!("已是最新，无需提示"),
                Some(candidate) => println!(
                    "候选版本: {}  → 启动时会弹窗: {}",
                    candidate.version,
                    if report.should_prompt() {
                        "是"
                    } else {
                        "否(已被忽略)"
                    }
                ),
            }
            println!(
                "安装命令: npm {}",
                updates::install_argv(
                    report
                        .candidate
                        .as_ref()
                        .map(|c| c.version.as_str())
                        .unwrap_or("<version>")
                )
                .join(" ")
            );
        }
    }
}
