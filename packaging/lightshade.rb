class Lightshade < Formula
  desc "Rust-native embedded database with generated storage and queries"
  homepage "https://github.com/burak-colakkadioglu/lightshadedb"
  url "https://crates.io/api/v1/crates/lightshadedb/2.2.0/download"
  sha256 "REPLACE_WITH_CRATES_IO_SHA256_FOR_2_1_2"
  license any_of: ["MIT", "Apache-2.0"]
  depends_on "rust" => :build

  def install
    system "cargo", "install", "--locked", "--root", prefix, "--path", "."
    bin.install Dir["#{prefix}/bin/*"]
  end

  test do
    assert_match "lightshade 2.2.0", shell_output("#{bin}/lightshade --version")
    (testpath/"messages.lightshade").write("table messages {\n    id: i64 primary_key\n    body: text\n}\n\noperation insert_message(id: i64, body: text) {\n    insert messages\n}\n")
    system "#{bin}/lightshade", "check", testpath/"messages.lightshade"
  end
end
