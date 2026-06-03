# typed: true
# frozen_string_literal: true

# HyperAgent — Ultra-fast CLI coding agent
# Homebrew Formula
#
# Install:
#   brew tap your-org/hyperagent
#   brew install hyperagent
#
# Or directly:
#   brew install your-org/hyperagent/hyperagent

class Hyperagent < Formula
  desc "Ultra-fast CLI coding agent with multi-agent pipeline and code understanding"
  homepage "https://github.com/nick-zhang-1991/HyperAgent"
  license "MIT"
  version "0.1.0"

  if OS.mac?
    if Hardware::CPU.arm?
      url "https://github.com/nick-zhang-1991/HyperAgent/releases/latest/download/hyperagent-macos-aarch64.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000" # Update on release
    else
      url "https://github.com/nick-zhang-1991/HyperAgent/releases/latest/download/hyperagent-macos-x86_64.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  elsif OS.linux?
    if Hardware::CPU.arm?
      url "https://github.com/nick-zhang-1991/HyperAgent/releases/latest/download/hyperagent-linux-aarch64.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    else
      url "https://github.com/nick-zhang-1991/HyperAgent/releases/latest/download/hyperagent-linux-x86_64.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  def install
    bin.install "hyperagent" => "hyper"
  end

  test do
    assert_match "HyperAgent", shell_output("#{bin}/hyper --version")
  end
end
