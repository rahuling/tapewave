param(
    [switch]$Installer,
    [string]$IsccPath = ""
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    cargo test --workspace --locked -j 2 --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw "Tests failed" }
    cargo build --release --locked --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw "Release build failed" }

    $version = (Select-String -Path Cargo.toml -Pattern '^version = "([^"]+)"').Matches[0].Groups[1].Value
    New-Item -ItemType Directory -Force -Path dist | Out-Null
    Compress-Archive -Path target/x86_64-pc-windows-msvc/release/tapewave.exe,LICENSE -DestinationPath "dist/Tapewave-$version-windows-x64-portable.zip" -Force

    if ($Installer) {
        if (-not $IsccPath) {
            $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
            if ($command) { $IsccPath = $command.Source }
            else {
                $candidates = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe")
                $IsccPath = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
            }
        }
        if (-not $IsccPath -or -not (Test-Path $IsccPath)) {
            throw "Inno Setup 6 is required for the installer. Install it from https://jrsoftware.org/isinfo.php or supply -IsccPath. The portable ZIP is ready in dist/."
        }
        & $IsccPath "/DAppVersion=$version" packaging/windows/tapewave.iss
        if ($LASTEXITCODE -ne 0) { throw "Installer compilation failed" }
    }
    Write-Output "Packages are in $root\dist"
} finally {
    Pop-Location
}
