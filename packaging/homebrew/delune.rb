# Homebrew formula for delune, installed from the release binaries.
# Lives in a tap (github.com/PndaMan/homebrew-delune); scripts/bump-homebrew.sh
# fills in the version and checksums for a release.
class Delune < Formula
  desc "Find music on Soulseek, review it, and add it to your Navidrome library"
  homepage "https://github.com/PndaMan/delune"
  version "0.1.0"
  license "AGPL-3.0-only"

  on_macos do
    on_arm do
      url "https://github.com/PndaMan/delune/releases/download/v#{version}/delune-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
    on_intel do
      url "https://github.com/PndaMan/delune/releases/download/v#{version}/delune-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/PndaMan/delune/releases/download/v#{version}/delune-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
    on_intel do
      url "https://github.com/PndaMan/delune/releases/download/v#{version}/delune-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  def install
    bin.install "delune"
  end

  service do
    run [opt_bin/"delune", "serve", "--data-dir", var/"delune"]
    keep_alive true
    log_path var/"log/delune.log"
    error_log_path var/"log/delune.log"
  end

  test do
    assert_match "delune", shell_output("#{bin}/delune --help")
  end
end
