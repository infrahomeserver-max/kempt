# Homebrew formula template. Put it in a tap (a repo named homebrew-kempt) and fill in url / sha256 from the release page:
#   brew tap infrahomeserver-max/kempt && brew install kempt
class Kempt < Formula
  desc "Open, search, fix, format and compare big JSON, XML and log files"
  homepage "https://github.com/infrahomeserver-max/kempt"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/infrahomeserver-max/kempt/releases/download/v0.1.0/kempt-0.1.0-macos-arm64.tar.gz"
      sha256 "REPLACE_WITH_SHA256_OF_THE_ARM64_TARBALL"
    end
    on_intel do
      url "https://github.com/infrahomeserver-max/kempt/releases/download/v0.1.0/kempt-0.1.0-macos-x86_64.tar.gz"
      sha256 "REPLACE_WITH_SHA256_OF_THE_X86_64_TARBALL"
    end
  end
  on_linux do
    on_intel do
      url "https://github.com/infrahomeserver-max/kempt/releases/download/v0.1.0/kempt-0.1.0-linux-x86_64.tar.gz"
      sha256 "REPLACE_WITH_SHA256_OF_THE_LINUX_X86_64_TARBALL"
    end
    on_arm do
      url "https://github.com/infrahomeserver-max/kempt/releases/download/v0.1.0/kempt-0.1.0-linux-aarch64.tar.gz"
      sha256 "REPLACE_WITH_SHA256_OF_THE_LINUX_AARCH64_TARBALL"
    end
  end

  def install
    bin.install "kempt"
  end

  test do
    assert_match "kempt", shell_output("#{bin}/kempt --version")
    (testpath/"t.json").write('{"a":1}')
    system bin/"kempt", "format", "t.json"
    assert_equal "{\n  \"a\": 1\n}\n", (testpath/"t.json").read
  end
end
