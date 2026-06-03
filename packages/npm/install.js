#!/usr/bin/env node
/**
 * HyperAgent npm installer
 *
 * Downloads the prebuilt binary for the current platform on postinstall.
 * Falls back to source build if binary is unavailable.
 *
 * Based on patterns from @anthropic-ai/claude-code and @openai/codex.
 */

const fs = require('fs');
const path = require('path');
const https = require('https');
const { spawn, execSync } = require('child_process');
const os = require('os');

const PACKAGE_VERSION = process.env.npm_package_version || '0.1.0';
const REPO = 'nick-zhang-1991/HyperAgent';

function getPlatform() {
  const arch = os.arch();
  const platform = os.platform();

  const archMap = {
    x64: 'x86_64',
    arm64: 'aarch64',
  };

  const platformMap = {
    darwin: 'apple-darwin',
    linux: 'unknown-linux-gnu',
  };

  const mappedArch = archMap[arch];
  const mappedPlatform = platformMap[platform];

  if (!mappedArch || !mappedPlatform) {
    return null;
  }

  return `${mappedArch}-${mappedPlatform}`;
}

function getTargetName(platform) {
  const names = {
    'x86_64-apple-darwin': 'hyperagent-macos-x86_64',
    'aarch64-apple-darwin': 'hyperagent-macos-aarch64',
    'x86_64-unknown-linux-gnu': 'hyperagent-linux-x86_64',
    'aarch64-unknown-linux-gnu': 'hyperagent-linux-aarch64',
  };
  return names[platform] || null;
}

function download(url, dest) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);
    const request = https.get(url, (response) => {
      if (response.statusCode === 302 || response.statusCode === 301) {
        file.close();
        fs.unlinkSync(dest);
        return download(response.headers.location, dest).then(resolve).catch(reject);
      }
      if (response.statusCode !== 200) {
        file.close();
        fs.unlinkSync(dest);
        reject(new Error(`HTTP ${response.statusCode}: ${response.statusMessage}`));
        return;
      }
      response.pipe(file);
      file.on('finish', () => {
        file.close();
        resolve();
      });
    });
    request.on('error', (err) => {
      file.close();
      fs.unlinkSync(dest);
      reject(err);
    });
    request.end();
  });
}

async function install() {
  const binDir = path.join(__dirname, 'bin');
  const binPath = path.join(binDir, 'hyper');

  // Create bin directory
  fs.mkdirSync(binDir, { recursive: true });

  const platform = getPlatform();
  if (!platform) {
    console.log(`⚠️  Unsupported platform: ${os.platform()} ${os.arch()}`);
    console.log('   Try building from source: cargo install hyperagent');
    process.exit(0);
  }

  const targetName = getTargetName(platform);
  if (!targetName) {
    console.log(`⚠️  No prebuilt binary for ${platform}`);
    console.log('   Try building from source: cargo install hyperagent');
    process.exit(0);
  }

  // Determine version tag
  const isDev = PACKAGE_VERSION.includes('-dev') || PACKAGE_VERSION === '0.0.0';
  const versionTag = isDev ? 'latest' : `v${PACKAGE_VERSION}`;

  // Try to download prebuilt binary
  const url = `https://github.com/${REPO}/releases/${versionTag === 'latest' ? 'latest/download' : 'download/' + versionTag}/${targetName}.tar.gz`;
  const tmpFile = path.join(os.tmpdir(), `hyperagent-${targetName}.tar.gz`);

  console.log(`📦 Downloading HyperAgent for ${platform}...`);
  console.log(`   ${url}`);

  try {
    await download(url, tmpFile);

    // Extract
    execSync(`tar xzf "${tmpFile}" -C "${binDir}"`, { stdio: 'pipe' });

    // The binary might be extracted directly or in a subdirectory
    const extractedBin = path.join(binDir, 'hyperagent');
    if (fs.existsSync(extractedBin)) {
      fs.renameSync(extractedBin, binPath);
    }

    // Cleanup
    fs.unlinkSync(tmpFile);

    // Make executable
    fs.chmodSync(binPath, 0o755);

    console.log(`✅ HyperAgent installed to ${binPath}`);
    console.log(`   Run: npx hyper`);
  } catch (err) {
    console.log(`⚠️  Binary download failed: ${err.message}`);
    console.log('   Falling back to source build...');
    console.log('   Install Rust: curl --proto \'=https\' --tlsv1.2 -sSf https://sh.rustup.rs | sh');
    process.exit(1);
  }
}

install().catch((err) => {
  console.error('❌ Installation failed:', err.message);
  process.exit(1);
});
