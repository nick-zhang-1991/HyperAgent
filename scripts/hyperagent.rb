class Hyperagent < Formula
  desc "Ultra-fast CLI coding agent with multi-agent parallelism"
  homepage "https://github.com/nick-zhang-1991/HyperAgent"
  version "0.2.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/nick-zhang-1991/HyperAgent/releases/download/v0.2.0/hyperagent-macos-aarch64.tar.gz"
      sha256 "REPLACE_WITH_ACTUAL_SHA256"
    else
      url "https://github.com/nick-zhang-1991/HyperAgent/releases/download/v0.2.0/hyperagent-macos-x86_64.tar.gz"
      sha256 "REPLACE_WITH_ACTUAL_SHA256"
    end
  end

  on_linux do
    url "https://github.com/nick-zhang-1991/HyperAgent/releases/download/v0.2.0/hyperagent-linux.tar.gz"
    sha256 "REPLACE_WITH_ACTUAL_SHA256"
  end

  def install
    bin.install "hyperagent" => "hyper"
    bin.install_symlink bin/"hyper" => "hyperagent"
  end

  test do
    system "#{bin}/hyper", "--version"
  end

  def caveats
    <<~EOS
      HyperAgent installed! 🚀

      Quick start:
        cd your-project
        hyper init
        hyper run "add error handling"

      Set up your LLM provider:
        export HYPER_LLM_API_KEY="sk-..."
        export HYPER_LLM_MODEL="gpt-4o"
        export HYPER_LLM_BASE_URL="https://api.openai.com/v1"

      Chinese UI (中文界面):
        export HYPER_LANG=zh-CN

      Web server:
        hyper serve
    EOS
  end
end
