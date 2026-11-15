cask "easy-download-manager" do
  arch arm: "aarch64", intel: "x64"

  version "0.10.0"
  sha256 arm:   "34f676644ba1d83971fbe82c853201ab87d920720bc8bd3dca7ef03a98febbb5",
         intel: "1a1bb99e0f62c4dbe6f142c31e3494d0d9a3d780bf5bc0501078f88b92fcf7c8"

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
