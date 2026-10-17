$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    & cargo test
    if ($LASTEXITCODE -ne 0) { throw 'Application tests failed' }
    & cargo test --no-default-features
    if ($LASTEXITCODE -ne 0) { throw 'Headless tests failed' }
    & cargo clippy --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Static analysis failed' }
    & cargo clippy --all-targets --no-default-features -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Headless static analysis failed' }
} finally {
    Pop-Location
}
