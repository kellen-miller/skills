$ErrorActionPreference = 'Stop'
$skillDir = Split-Path -Parent $PSScriptRoot
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "dispatch-tickets requires Rust/Cargo; install the toolchain in $skillDir/rust-toolchain.toml"
}

$cacheRoot = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { Join-Path $HOME '.cache' }
$sha = [System.Security.Cryptography.SHA256]::Create()
try {
    $installation = [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($skillDir))).Replace('-', '').ToLowerInvariant()
} finally {
    $sha.Dispose()
}

$targetDir = Join-Path $cacheRoot "agent-skills/dispatch-tickets/$installation"
$originalToolchain = $env:RUSTUP_TOOLCHAIN
Push-Location $skillDir
try {
    $env:RUSTUP_TOOLCHAIN = (Select-String -Path (Join-Path $skillDir 'rust-toolchain.toml') -Pattern '^channel = "([^"]+)"').Matches.Groups[1].Value
    $compiler = & rustc -vV
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $hostTarget = ($compiler | Select-String -Pattern '^host: (.+)').Matches.Groups[1].Value
    & cargo build --target $hostTarget --quiet --locked --manifest-path (Join-Path $skillDir 'Cargo.toml') --target-dir $targetDir
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
    Pop-Location
    $env:RUSTUP_TOOLCHAIN = $originalToolchain
}

& (Join-Path $targetDir "$hostTarget/debug/dispatch-tickets.exe") @args
exit $LASTEXITCODE
