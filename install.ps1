# Install grove on Windows: https://github.com/AgentsORG/grove
#
#   irm https://raw.githubusercontent.com/AgentsORG/grove/main/install.ps1 | iex
#
# Settings: $env:GROVE_VERSION (a tag like v0.1.0, default latest),
#           $env:GROVE_INSTALL_DIR (default $HOME\.grove\bin)
$ErrorActionPreference = 'Stop'

$repo = if ($env:GROVE_REPO) { $env:GROVE_REPO } else { 'AgentsORG/grove' }
$version = if ($env:GROVE_VERSION) { $env:GROVE_VERSION } else { 'latest' }
$installDir = if ($env:GROVE_INSTALL_DIR) { $env:GROVE_INSTALL_DIR } else { Join-Path $HOME '.grove\bin' }

$cpu = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
$target = "$cpu-pc-windows-msvc"
$asset = "grove-$target.zip"
$base = if ($env:GROVE_DOWNLOAD_BASE) { $env:GROVE_DOWNLOAD_BASE } elseif ($version -eq 'latest') { "https://github.com/$repo/releases/latest/download" } else { "https://github.com/$repo/releases/download/$version" }

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("grove-install-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force $tmp | Out-Null
try {
    Write-Host "Downloading grove for $target ($version)"
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -UseBasicParsing "$base/$asset" -OutFile (Join-Path $tmp $asset)

    try {
        Invoke-WebRequest -UseBasicParsing "$base/$asset.sha256" -OutFile (Join-Path $tmp "$asset.sha256")
        $expected = ((Get-Content (Join-Path $tmp "$asset.sha256") -Raw).Trim() -split '\s+')[0].ToLower()
        $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $asset)).Hash.ToLower()
        if ($expected -ne $actual) { throw "checksum mismatch for $asset" }
    } catch [System.Net.WebException] {
        Write-Host 'No checksum published for this release; skipping verification.'
    }

    Expand-Archive -Force (Join-Path $tmp $asset) $tmp
    $bin = Get-ChildItem -Path $tmp -Recurse -Filter 'grove.exe' | Select-Object -First 1
    if (-not $bin) { throw 'the download did not contain grove.exe' }
    New-Item -ItemType Directory -Force $installDir | Out-Null
    Copy-Item -Force $bin.FullName (Join-Path $installDir 'grove.exe')
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not $env:GROVE_NO_MODIFY_PATH -and -not ($userPath -split ';' | Where-Object { $_ -eq $installDir })) {
    [Environment]::SetEnvironmentVariable('Path', ($userPath.TrimEnd(';') + ";$installDir"), 'User')
    $env:Path = "$env:Path;$installDir"
    Write-Host "Added $installDir to your PATH (new terminals pick it up)."
}
& (Join-Path $installDir 'grove.exe') --version
Write-Host ''
Write-Host 'For agents: npx skills add AgentsORG/grove'
