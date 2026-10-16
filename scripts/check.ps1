param([switch]$WithoutVoice)
$ErrorActionPreference = 'Stop'
$originalPath = $env:PATH
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    if (-not $WithoutVoice -and -not (Get-Command cmake -ErrorAction SilentlyContinue)) {
        $candidates = @(
            'C:\Program Files\CMake\bin',
            'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin'
        )
        foreach ($candidate in $candidates) {
            if (Test-Path (Join-Path $candidate 'cmake.exe')) {
                $env:PATH = "$candidate;$env:PATH"
                break
            }
        }
        if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
            throw 'Voice builds require CMake on PATH. Install CMake or use -WithoutVoice.'
        }
    }
    $featureArgs = @(if ($WithoutVoice) { '--no-default-features'; '--features'; 'gui' } else { '--all-features' })
    & cargo test @featureArgs
    if ($LASTEXITCODE -ne 0) { throw 'Application tests failed' }
    & cargo test --no-default-features
    if ($LASTEXITCODE -ne 0) { throw 'Headless tests failed' }
    & cargo clippy @featureArgs -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Static analysis failed' }
} finally {
    $env:PATH = $originalPath
    Pop-Location
}
