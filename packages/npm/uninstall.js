#!/usr/bin/env node
/**
 * HyperAgent npm uninstaller
 *
 * Removes the downloaded binary on npm uninstall.
 */
const fs = require('fs');
const path = require('path');

const binPath = path.join(__dirname, 'bin', 'hyper');
const binDir = path.join(__dirname, 'bin');

if (fs.existsSync(binPath)) {
  fs.unlinkSync(binPath);
  console.log('🧹 Removed HyperAgent binary');
}
if (fs.existsSync(binDir)) {
  fs.rmdirSync(binDir);
}
