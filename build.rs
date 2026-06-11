fn main() {
    let _ = embed_resource::compile("assets/icon.rc", embed_resource::NONE);

    // 注入版本号：CARGO_PKG_VERSION + git short hash
    let pkg_version = env!("CARGO_PKG_VERSION");
    let git_hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let build_version = format!("{}-{}", pkg_version, git_hash);
    println!("cargo:rustc-env=BUILD_VERSION={}", build_version);
    // 重新构建条件：git HEAD 变化时重新编译
    println!("cargo:rerun-if-changed=.git/HEAD");
}
