param(
    [string]$OutputDirectory = (Join-Path $env:TEMP 'tapewave-ui-captures')
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
    cargo run --locked -- --ui-smoke-test $OutputDirectory
    if ($LASTEXITCODE -ne 0) { throw 'Native renderer smoke test failed' }
    $expected = @('music','radio','podcasts','appearance','paper','compact-queue','add-station','empty','compact-appearance')
    foreach ($name in $expected) {
        $path = Join-Path $OutputDirectory "$name.png"
        if (-not (Test-Path $path)) { throw "Missing capture: $path" }
    }
    Write-Output "Native renderer captures are in $OutputDirectory"
} finally {
    Pop-Location
}
