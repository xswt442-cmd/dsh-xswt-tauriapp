//! Diagnostic: print what this shell would see on the machine, and what the
//! update dialog would offer.
//!
//! ```text
//! cargo run --example check-updates        # detect everything
//! cargo run --example check-updates 0.1.5-rc.1   # pretend this is installed
//! ```

use dsh_xswt_tauriapp_core::{paths, ports, server, updates};

fn main() {
    println!("== 环境 ==");
    println!("DSH_HOME       : {}", paths::dsh_home().display());
    match paths::resolve_node() {
        Some(node) => println!("node           : {}", node.display()),
        None => println!("node           : ✗ 未找到"),
    }
    match paths::resolve_dsh_bin() {
        Some(bin) => println!("dsh launcher   : {}", bin.display()),
        None => println!("dsh launcher   : ✗ 未找到"),
    }
    match paths::npm_for_dsh() {
        Some(npm) => println!("npm 装 dsh 的  : {}", npm.display()),
        None => println!("npm 装 dsh 的  : ✗ 未找到"),
    }
    match paths::installed_version() {
        Some(version) => println!("dsh 版本       : {version}"),
        None => println!("dsh 版本       : ✗ 无法读取"),
    }
    println!("日志目录       : {}", paths::log_dir().display());

    println!(
        "\n== 端口扫描 ({}–{}) ==",
        ports::START_PORT,
        ports::MAX_PORT
    );
    match server::find_running_url() {
        Some((port, url)) => println!("已发现运行中的服务: :{port} → {url}"),
        None => println!("未发现运行中的 dsh 服务"),
    }
    match ports::find_free_port() {
        Some(port) => println!("首个空闲端口   : {port}"),
        None => println!("✗ 端口段全被占用"),
    }

    let current = std::env::args()
        .nth(1)
        .or_else(paths::installed_version)
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
                .map(|argv| argv.join(" "))
                .unwrap_or_else(|error| format!("<{error}>"))
            );

            // The command as the update button builds it on this platform: which
            // npm, and through what. Printed, never run — the moment this reaches
            // npm it is an install, not a diagnostic.
            println!("\n== 本平台的安装命令（未执行）==");
            match report.candidate.as_ref() {
                None => println!("(无候选版本)"),
                Some(candidate) => match paths::npm_for_dsh() {
                    None => println!("✗ 没有找到能安装 dsh 的 npm"),
                    Some(npm) => {
                        match updates::install_command(
                            &npm,
                            &candidate.version,
                            std::env::consts::OS,
                        ) {
                            Ok(command) => {
                                println!("{} {}", command.program.display(), command.args.join(" "))
                            }
                            Err(error) => println!("✗ {error}"),
                        }
                    }
                },
            }
        }
    }
}
