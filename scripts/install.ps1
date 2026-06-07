<#
.SYNOPSIS
    HyperAgent Windows Installer — 一键安装 HyperAgent 到 Windows
.DESCRIPTION
    从 GitHub Releases 下载预编译二进制或从源码构建。
    - Binary 模式: 下载对应架构的 release
    - Source 模式: 要求已安装 Rust (通过 rustup)
.PARAMETER Version
    指定版本 (默认: latest)
.PARAMETER Build
    从源码构建 (需要 Rust 工具链)
.PARAMETER Binary
    强制使用预编译二进制下载
.EXAMPLE
    .\install.ps1
    .\install.ps1 -Version v0.1.0
    .\install.ps1 -Build
#>

param(
    [string]$Version = "latest",
    [switch]$Build = $false,
    [switch]$Binary = $false
)

$ErrorActionPreference = "Stop"

# Colors (PowerShell 5.1+ supports ANSI on Win10+)
$Cyan = "$([char]0x1b)[36m"
$Green = "$([char]0x1b)[32m"
$Yellow = "$([char]0x1b)[33m"
$Red = "$([char]0x1b)[31m"
$NC = "$([char]0x1b)[0m"

Write-Host "${Cyan}╔══════════════════════════════════════╗${NC}"
Write-Host "${Cyan}║     HyperAgent Windows Installer     ║${NC}"
Write-Host "${Cyan}╚══════════════════════════════════════╝${NC}"
Write-Host ""

# ─── Platform detection ──────────────────────────────────────────
$Arch = (Get-WmiObject Win32_Processor | Select-Object -First 1).AddressWidth
if ($Arch -eq 64) {
    $Target = "x86_64-pc-windows-msvc"
    $ArchName = "x86_64"
} elseif ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") {
    $Target = "aarch64-pc-windows-msvc"
    $ArchName = "aarch64"
} else {
    Write-Host "${Red}❌ Unsupported architecture: $Arch${NC}"
    exit 1
}

Write-Host "${Yellow}🔍 Detected: Windows / $ArchName ($Target)${NC}"
Write-Host ""

# ─── Binary dir ──────────────────────────────────────────────────
$BinDir = "$env:USERPROFILE\.hyper\bin"
if (-not (Test-Path $BinDir)) {
    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
}

$InstallMode = "auto"
if ($Build) { $InstallMode = "source" }
if ($Binary) { $InstallMode = "binary" }

# ─── Binary install ──────────────────────────────────────────────
if ($InstallMode -ne "source") {
    Write-Host "${Yellow}📦 Downloading pre-built binary...${NC}"

    $Owner = "your-org"  # TODO: set to actual org/repo
    $Repo = "hyperagent"

    if ($Version -eq "latest") {
        $DownloadUrl = "https://github.com/${Owner}/${Repo}/releases/latest/download/${Repo}-${Target}.zip"
    } else {
        $DownloadUrl = "https://github.com/${Owner}/${Repo}/releases/download/${Version}/${Repo}-${Target}.zip"
    }

    $TempDir = [System.IO.Path]::GetTempPath()
    $ZipFile = Join-Path $TempDir "hyperagent.zip"

    try {
        Write-Host "   Downloading from: $DownloadUrl"
        Invoke-WebRequest -Uri $DownloadUrl -OutFile $ZipFile -UserAgent "HyperAgent/installer"
        Write-Host "   ${Green}✅ Downloaded binary${NC}"

        # Extract
        $ExtractDir = Join-Path $TempDir "hyperagent-extract"
        if (Test-Path $ExtractDir) { Remove-Item -Recurse -Force $ExtractDir }
        Expand-Archive -Path $ZipFile -DestinationPath $ExtractDir -Force

        # Find hyper.exe / hyperagent.exe
        $BinaryFound = $false
        $PossiblePaths = @(
            Join-Path $ExtractDir "hyper.exe",
            Join-Path $ExtractDir "hyperagent.exe",
            Join-Path $ExtractDir "release\hyper.exe",
            Join-Path $ExtractDir "release\hyperagent.exe"
        )
        foreach ($p in $PossiblePaths) {
            if (Test-Path $p) {
                Copy-Item $p (Join-Path $BinDir "hyper.exe") -Force
                $BinaryFound = $true
                Write-Host "   ${Green}✅ Installed to $BinDir\hyper.exe${NC}"
                break
            }
        }

        # Clean up
        Remove-Item $ZipFile -Force -ErrorAction SilentlyContinue
        Remove-Item -Recurse -Force $ExtractDir -ErrorAction SilentlyContinue

        if (-not $BinaryFound) {
            Write-Host "${Yellow}   ⚠️  Binary layout unknown, falling back to source build${NC}"
            $InstallMode = "source"
        }
    } catch {
        Write-Host "${Yellow}   ⚠️  Binary download failed: $_${NC}"
        $InstallMode = "source"
    }
}

# ─── Source build ────────────────────────────────────────────────
if ($InstallMode -eq "source" -or $InstallMode -eq "auto") {
    Write-Host "${Yellow}🔧 Building from source...${NC}"

    # Check Rust
    $Rustc = Get-Command "rustc" -ErrorAction SilentlyContinue
    if (-not $Rustc) {
        Write-Host "${Red}❌ Rust not found.${NC}"
        Write-Host "   Install: https://rustup.rs"
        Write-Host "   Or run: winget install Rustlang.Rustup"
        exit 1
    }

    $ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
    $ProjectDir = Resolve-Path (Join-Path $ScriptDir "..")

    if (-not (Test-Path (Join-Path $ProjectDir "Cargo.toml"))) {
        Write-Host "${Red}❌ Must run install.ps1 from the hyperagent project root.${NC}"
        Write-Host "   cd hyperagent && .\scripts\install.ps1"
        exit 1
    }

    Set-Location $ProjectDir
    Write-Host "   Building release binary (this may take a few minutes)..."
    $BuildResult = cargo build --release 2>&1 | Select-Object -Last 1
    if ($LASTEXITCODE -ne 0) {
        Write-Host "${Red}❌ Build failed${NC}"
        Write-Host "   $BuildResult"
        exit 1
    }
    Write-Host "   ${Green}✅ Build complete${NC}"

    if (Test-Path "target\release\hyperagent.exe") {
        Copy-Item "target\release\hyperagent.exe" (Join-Path $BinDir "hyper.exe") -Force
    }
}

# ─── Verify ──────────────────────────────────────────────────────
Write-Host ""
Write-Host "${Yellow}🧪 Verifying installation...${NC}"

$HyperPath = Join-Path $BinDir "hyper.exe"
if (Test-Path $HyperPath) {
    $VersionOutput = & $HyperPath --version 2>&1
    Write-Host "   ${Green}✅ Installed: $VersionOutput${NC}"
} else {
    Write-Host "${Red}❌ Installation verification failed${NC}"
    Write-Host "   Binary not found at: $HyperPath"
    exit 1
}

# ─── PATH check ──────────────────────────────────────────────────
$CurrentPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($CurrentPath -notlike "*$BinDir*") {
    Write-Host "${Yellow}   ⚠️  $BinDir not in PATH. Add to user PATH:${NC}"
    Write-Host "      [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path','User') + ';$BinDir', 'User')"
}

# ─── Config check ────────────────────────────────────────────────
$ConfigDir = "$env:USERPROFILE\.config\hyper"
if (-not (Test-Path "$ConfigDir\config.toml")) {
    Write-Host ""
    Write-Host "${Yellow}⚙️  First-time setup:${NC}"
    Write-Host "   Run: hyper config-init"
    Write-Host "   Then set API key in: $ConfigDir\config.toml"
    Write-Host "   Or use env vars: `$env:HYPER_LLM_API_KEY='sk-...'"
}

Write-Host ""
Write-Host "${Cyan}╔══════════════════════════════════════╗${NC}"
Write-Host "${Cyan}║     HyperAgent Ready! 🚀             ║${NC}"
Write-Host "${Cyan}╚══════════════════════════════════════╝${NC}"
Write-Host ""
Write-Host "   Quick start:"
Write-Host "     cd your-project"
Write-Host "     hyper init"
Write-Host "     hyper run `"add error handling`""
Write-Host "     hyper run `"explain this`" --mode ask"
Write-Host "     hyper diff --side-by-side"
Write-Host ""
