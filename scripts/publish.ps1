<#
.SYNOPSIS
    自动发布 rust-agent-framework workspace 下所有 crate 到 crates.io。
.DESCRIPTION
    按依赖拓扑顺序依次发布：
        macros → core → client → framework → workflow → cli
    所有 crate 使用统一的 workspace 版本号。
.PARAMETER Version
    指定要发布的版本号。如果不提供，则使用 Cargo.toml 中当前的 workspace 版本。
.PARAMETER DryRun
    使用 --dry-run 模式，只验证不真正发布。
.PARAMETER SkipVersionCheck
    跳过版本号确认提示。
.EXAMPLE
    # 使用当前版本发布（会提示确认）
    ./scripts/publish.ps1

    # 指定版本并发布
    ./scripts/publish.ps1 -Version "0.2.0"

    # Dry-run 模式验证
    ./scripts/publish.ps1 -DryRun
#>

param(
    [string]$Version,
    [switch]$DryRun,
    [switch]$SkipVersionCheck
)

$ErrorActionPreference = "Stop"
$PROJECT_ROOT = Resolve-Path "$PSScriptRoot/.."
Set-Location $PROJECT_ROOT

# ---- 颜色输出辅助 ----
function Write-Success { Write-Host "[✓] $args" -ForegroundColor Green }
function Write-Info    { Write-Host "[i] $args" -ForegroundColor Cyan }
function Write-Warn    { Write-Host "[!] $args" -ForegroundColor Yellow }
function Write-Error   { Write-Host "[✗] $args" -ForegroundColor Red }

# ---- 1. 前置检查 ----
Write-Info "=== 前置检查 ==="

# 检查 cargo 是否可用
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "未找到 cargo，请先安装 Rust 工具链。"
    exit 1
}
Write-Success "cargo 可用"

# 检查 crates.io 登录状态（通过检查凭据文件）
$credPath = "$env:USERPROFILE\.cargo\credentials.toml"
if (-not (Test-Path $credPath)) {
    Write-Warn "未检测到 crates.io 登录凭据，尝试执行 'cargo login'……"
    cargo login
    if ($LASTEXITCODE -ne 0) {
        Write-Error "登录失败，请手动执行 'cargo login' 后重试。"
        exit 1
    }
}
Write-Success "crates.io 登录状态正常"

# ---- 2. 版本处理 ----
Write-Info "=== 版本处理 ==="

# 读取当前 workspace 版本
$cargoToml = Get-Content -Path "$PROJECT_ROOT/Cargo.toml" -Raw
$currentVersion = if ($cargoToml -match 'version\s*=\s*"([^"]+)"') { $matches[1] } else { "未知" }

if (-not $Version) {
    $Version = $currentVersion
    Write-Info "使用当前 workspace 版本: $Version"
} else {
    Write-Info "指定版本: $Version"
}

if (-not $SkipVersionCheck -and -not $DryRun) {
    $confirm = Read-Host "确认发布版本 v$Version 到 crates.io? (y/N)"
    if ($confirm -ne "y" -and $confirm -ne "Y") {
        Write-Warn "已取消发布。"
        exit 0
    }
}

# ---- 3. 如果需要，更新版本号 ----
if ($Version -ne $currentVersion) {
    Write-Info "更新 workspace 版本号: $currentVersion → $Version"
    $newContent = $cargoToml -replace '^version\s*=\s*"[^"]+"', ('version = "' + $Version + '"')
    Set-Content -Path "$PROJECT_ROOT/Cargo.toml" -Value $newContent -NoNewline
    Write-Success "版本已更新为 $Version"
}

# ---- 4. 按依赖顺序发布 ----
$PUBLISH_ORDER = @(
    @{ name = "rust-agent-macros";    path = "crates/macros" },
    @{ name = "rust-agent-core";      path = "crates/core" },
    @{ name = "rust-agent-client";    path = "crates/client" },
    @{ name = "rust-agent-framework"; path = "crates/framework" },
    @{ name = "rust-agent-workflow";  path = "crates/workflow" },
    @{ name = "rust-agent-cli";       path = "crates/cli" }
)

Write-Info "=== 开始发布 v$Version ==="

$dryRunFlag = if ($DryRun) { "--dry-run" } else { "" }

foreach ($crate in $PUBLISH_ORDER) {
    $name = $crate.name
    $path = $crate.path
    Write-Info "正在发布 [$name] ..."

    $publishCmd = "cargo publish -p $name --registry crates-io --allow-dirty $dryRunFlag"
    if ($DryRun) {
        Write-Info "  Dry-run 模式，执行: $publishCmd"
    }

    $output = Invoke-Expression $publishCmd 2>&1
    $exitCode = $LASTEXITCODE

    if ($exitCode -eq 0) {
        Write-Success "[$name] 发布成功"
    } else {
        $outputText = $output -join "`n"
        # 检查是否是因为已存在相同版本而失败
        if ($outputText -match "crate version.*is already uploaded" -or $outputText -match "already exists") {
            Write-Warn "[$name] 版本 $Version 已发布，跳过"
        } else {
            Write-Error "[$name] 发布失败!"
            Write-Host $outputText -ForegroundColor Red
            if (-not $DryRun) {
                Write-Warn "后续 crate 发布已终止。"
            }
            exit 1
        }
    }
}

Write-Success "=== 所有 crate 发布完成 (v$Version) ==="

if ($DryRun) {
    Write-Info "本次为 Dry-run 模式，未实际发布任何内容。"
}