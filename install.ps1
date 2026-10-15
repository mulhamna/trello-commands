# install.ps1 — installer for trelloc (Unofficial Trello CLI + MCP server) on Windows
#
# Usage:
#   irm https://raw.githubusercontent.com/mulhamna/trello-commands/main/install.ps1 | iex
#
# Or with options:
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/mulhamna/trello-commands/main/install.ps1))) -Version v1.0.0
#
# Downloads the release zip for your architecture, verifies it against the
# release's checksums.txt, installs trelloc.exe and adds it to the user PATH.
# Not affiliated with or endorsed by Trello.

param(
    [string]$Version = "",
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\trelloc\bin",
    [switch]$SkipPathUpdate
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Repo = "mulhamna/trello-commands"

function Write-Info($msg) { Write-Host "==> $msg" -ForegroundColor Green }

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { "x86_64" }
    "ARM64" { "aarch64" }
    default { throw "Unsupported Windows architecture '$($env:PROCESSOR_ARCHITECTURE)'. Supported: AMD64, ARM64." }
}

$tag = if ($Version) { $Version } else {
    (Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest").tag_name
}
if (-not $tag) { throw "Could not determine the latest release." }
if (-not $tag.StartsWith("v")) { $tag = "v$tag" }

$name = "trelloc-windows-$arch"
$zip = "$name.zip"
$base = "https://github.com/$Repo/releases/download/$tag"

Write-Info "Installing trelloc $tag ($name) to $InstallDir"

$temp = Join-Path ([System.IO.Path]::GetTempPath()) ("trelloc-install-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $temp | Out-Null
try {
    $zipPath = Join-Path $temp $zip
    $sumsPath = Join-Path $temp "checksums.txt"
    Invoke-WebRequest -Uri "$base/$zip" -OutFile $zipPath -UseBasicParsing
    Invoke-WebRequest -Uri "$base/checksums.txt" -OutFile $sumsPath -UseBasicParsing

    $expected = Get-Content $sumsPath |
        ForEach-Object { $parts = $_ -split '\s+'; if ($parts.Count -ge 2 -and ($parts[1] -eq $zip -or $parts[1].EndsWith("/$zip"))) { $parts[0] } } |
        Select-Object -First 1
    if (-not $expected) { throw "No checksum for $zip in checksums.txt" }
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $zipPath).Hash.ToLowerInvariant()
    if ($actual -ne $expected.ToLowerInvariant()) {
        throw "Checksum mismatch for ${zip}: expected $expected, got $actual"
    }
    Write-Info "Checksum verified"

    $extract = Join-Path $temp "extract"
    Expand-Archive -LiteralPath $zipPath -DestinationPath $extract -Force
    $exe = Get-ChildItem -Path $extract -Recurse -Filter "$name.exe" | Select-Object -First 1
    if (-not $exe) { throw "Archive did not contain $name.exe" }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $target = Join-Path $InstallDir "trelloc.exe"
    Copy-Item -LiteralPath $exe.FullName -Destination $target -Force

    Write-Host ""
    Write-Host "trelloc $tag installed to $target" -ForegroundColor Green

    if ($SkipPathUpdate) {
        Write-Host "Skipped PATH update (-SkipPathUpdate)." -ForegroundColor DarkGray
    } else {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $entries = if ($userPath) { $userPath.Split(';', [System.StringSplitOptions]::RemoveEmptyEntries) } else { @() }
        if ($entries -notcontains $InstallDir) {
            $newPath = if ($userPath) { "$userPath;$InstallDir" } else { $InstallDir }
            [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
            Write-Host "Added $InstallDir to your user PATH. Restart the terminal to use it." -ForegroundColor Yellow
        }
    }

    Write-Host ""
    Write-Host "  Next: trelloc auth login"
    Write-Host "  MCP:  trelloc mcp install --client claude-code"
    Write-Host "  Docs: https://github.com/$Repo"
}
finally {
    if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Recurse -Force }
}
