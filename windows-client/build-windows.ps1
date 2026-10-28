$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    # Build on Windows with Rust's MSVC toolchain and Visual C++ Build Tools.
    # Statically link the CRT so end users do not need a VC redistributable.
    $env:RUSTFLAGS = '-C target-feature=+crt-static'
    cargo build --locked --release --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Windows build failed' }
    Write-Host "EXE: $PSScriptRoot\target\x86_64-pc-windows-msvc\release\jpeg-clipboard-windows.exe"
} finally {
    Pop-Location
}
