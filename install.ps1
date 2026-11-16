# Install ibkit (ibkit.exe and the JVM agent it loads into IB Gateway) from a
# GitHub release on Windows x64:
#
#   powershell -ExecutionPolicy ByPass -c "irm https://raw.githubusercontent.com/racetozero/ibkit/main/install.ps1 | iex"
#
# IBKIT_VERSION      release to install (default: the latest)
# IBKIT_INSTALL_DIR  directory for ibkit.exe and the agent (default: ~\.local\bin)
# IBKIT_REPOSITORY, IBKIT_RELEASES_URL  another release source (with
#                    IBKIT_RELEASES_URL, set IBKIT_VERSION too)
[CmdletBinding()]
param(
    [string]$Version = $env:IBKIT_VERSION,
    [string]$Target = $env:IBKIT_TARGET,
    [string]$InstallDirectory = $env:IBKIT_INSTALL_DIR,
    [string]$Repository = $env:IBKIT_REPOSITORY,
    [string]$ReleasesUrl = $env:IBKIT_RELEASES_URL
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version 3.0

function Fail([string]$Message) {
    throw "ibkit installer: $Message"
}

function Test-GitHubCli {
    if ($ReleasesUrl -or -not (Get-Command gh -ErrorAction SilentlyContinue)) {
        return $false
    }
    & gh auth status --hostname github.com *> $null
    return $LASTEXITCODE -eq 0
}

function Receive-File([string]$Source, [string]$Destination) {
    $Uri = [Uri]$Source
    if ($Uri.Scheme -eq "file") {
        Copy-Item -LiteralPath $Uri.LocalPath -Destination $Destination
        return
    }
    if ($Uri.Scheme -ne "https") {
        Fail "IBKIT_RELEASES_URL is $ReleasesUrl. Use an https:// or file:// URL."
    }
    Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $Destination
}

# Move a file into place through a staged copy. A loaded agent DLL cannot be
# overwritten, but it can be renamed, so a running Gateway keeps its copy.
function Install-File([string]$Source, [string]$Directory, [string]$Name) {
    $Destination = Join-Path $Directory $Name
    $Staged = Join-Path $Directory (".ibkit.install." + [Guid]::NewGuid())
    Copy-Item -LiteralPath $Source -Destination $Staged
    if (Test-Path -LiteralPath $Destination) {
        $Old = "$Destination.old-" + [Guid]::NewGuid()
        Move-Item -LiteralPath $Destination -Destination $Old
        Remove-Item -Force -ErrorAction SilentlyContinue -LiteralPath $Old
    }
    Move-Item -LiteralPath $Staged -Destination $Destination
}

try {
    if (-not $Repository) {
        $Repository = "racetozero/ibkit"
    }
    $UseGitHubCli = Test-GitHubCli
    if (-not $ReleasesUrl) {
        $ReleasesUrl = "https://github.com/$Repository/releases"
    }
    if (-not $Version -or $Version -eq "latest") {
        if ($UseGitHubCli) {
            $Version = (& gh release view --repo $Repository --json tagName --jq .tagName).Trim()
            if ($LASTEXITCODE -ne 0 -or -not $Version) {
                Fail "gh could not find the latest release of $Repository. Set IBKIT_VERSION, or check gh auth status."
            }
        } elseif ($env:IBKIT_RELEASES_URL) {
            Fail "IBKIT_RELEASES_URL is set without IBKIT_VERSION. Set IBKIT_VERSION to the release to install."
        } else {
            try {
                $Version = (Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases/latest").tag_name
            } catch {
                Fail "could not read the latest release from GitHub. Check the network connection, or set IBKIT_VERSION."
            }
        }
    }
    $Tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
    $PlainVersion = $Tag.Substring(1)
    if (-not $PlainVersion -or $PlainVersion -notmatch '^[0-9A-Za-z.+-]+$') {
        Fail "IBKIT_VERSION is $Version. Use a release version such as 1.2.3, or unset it for the latest release."
    }

    if (-not $Target) {
        $Architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
        if ($Architecture -ne "X64") {
            Fail "this processor is $Architecture. IB Gateway and the ibkit agent are for Windows x64."
        }
        $Target = "x86_64-pc-windows-msvc"
    }
    if ($Target -ne "x86_64-pc-windows-msvc") {
        Fail "IBKIT_TARGET is $Target. Use x86_64-pc-windows-msvc."
    }
    if (-not $InstallDirectory) {
        $InstallDirectory = Join-Path $HOME ".local\bin"
    }

    $ArchiveName = "ibkit-cli-$Target.zip"
    $ChecksumName = "$ArchiveName.sha256"
    $TemporaryDirectory = Join-Path ([IO.Path]::GetTempPath()) ("ibkit-install-" + [Guid]::NewGuid())
    New-Item -ItemType Directory -Path $TemporaryDirectory | Out-Null
    try {
        Write-Host "Installing ibkit $PlainVersion for $Target"
        if ($UseGitHubCli) {
            & gh release download $Tag --repo $Repository --pattern $ArchiveName --pattern $ChecksumName --dir $TemporaryDirectory --clobber
            if ($LASTEXITCODE -ne 0) {
                Fail "gh could not download $ArchiveName from release $Tag. Check that the release exists."
            }
        } else {
            $AssetBase = "$($ReleasesUrl.TrimEnd('/'))/download/$Tag"
            Receive-File "$AssetBase/$ArchiveName" (Join-Path $TemporaryDirectory $ArchiveName)
            Receive-File "$AssetBase/$ChecksumName" (Join-Path $TemporaryDirectory $ChecksumName)
        }

        $ArchivePath = Join-Path $TemporaryDirectory $ArchiveName
        $Expected = ((Get-Content -LiteralPath (Join-Path $TemporaryDirectory $ChecksumName) | Where-Object { $_.Trim() } | Select-Object -First 1) -split '\s+')[0]
        if ($Expected -notmatch '^[0-9A-Fa-f]{64}$') {
            Fail "$ChecksumName holds no SHA-256 checksum. Download the release again."
        }
        if ((Get-FileHash -Algorithm SHA256 -LiteralPath $ArchivePath).Hash -ne $Expected) {
            Fail "the checksum of $ArchiveName does not match $ChecksumName. Nothing was installed; download again."
        }

        Expand-Archive -LiteralPath $ArchivePath -DestinationPath $TemporaryDirectory
        $Unpacked = Join-Path $TemporaryDirectory "ibkit-cli-$Target"
        foreach ($Name in "ibkit.exe", "ibkit_gateway_agent.dll") {
            if (-not (Test-Path -LiteralPath (Join-Path $Unpacked $Name) -PathType Leaf)) {
                Fail "$ArchiveName does not hold $Name. Report this release at https://github.com/$Repository/issues."
            }
        }

        New-Item -ItemType Directory -Force -Path $InstallDirectory | Out-Null
        # The agent first: the new binary needs the new agent.
        Install-File (Join-Path $Unpacked "ibkit_gateway_agent.dll") $InstallDirectory "ibkit_gateway_agent.dll"
        Install-File (Join-Path $Unpacked "ibkit.exe") $InstallDirectory "ibkit.exe"
        Write-Host "Installed ibkit $PlainVersion and its agent to $InstallDirectory"
        if ($InstallDirectory -notin ($env:PATH -split ';')) {
            Write-Host "Add $InstallDirectory to PATH to run ibkit from any directory."
        }
    } finally {
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $TemporaryDirectory
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
