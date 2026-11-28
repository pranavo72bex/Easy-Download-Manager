cask "easy-download-manager" do
  arch arm: "aarch64", intel: "x64"

  version "0.10.3"
  sha256 arm:   "29d97593619d2278e0f92036395cf8753ed0a83423a1be2ed9e2691ed441ad7a",
         intel: "be46219a8af13e3fef09f2c85b602522251aa5c9c64573b45e5d7b523066e9ed"

  url "https://github.com/pranavo72bex/Easy-Download-Manager/releases/download/desktop-v#{version}/dm_#{version}_#{arch}.dmg"
  name "dm"
  desc "Download manager with many connections, resume and a queue"
  homepage "https://github.com/pranavo72bex/Easy-Download-Manager"

  livecheck do
    url :url
    regex(/^desktop[._-]v?(\d+(?:\.\d+)+)$/i)
    strategy :github_latest
  end

  depends_on macos: ">= :big_sur"

  app "dm.app"

  zap trash: "~/Library/Application Support/dm"
end
