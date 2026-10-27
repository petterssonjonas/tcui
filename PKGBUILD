pkgname=tcui
pkgver=0.6.0
pkgrel=1
pkgdesc="ChatGPT-style terminal chat UI"
arch=("x86_64")
url="https://github.com/petterssonjonas/tcui"
license=("GPL3")
depends=()
optdepends=(
  "tmux: open Edit in a tmux split when available"
  "libnotify: desktop notifications through notify-send"
)
makedepends=("cargo" "git" "rust")
source=("$pkgname::git+$url#tag=v$pkgver")
sha256sums=("SKIP")

prepare() {
  cd "$pkgname"
}

build() {
  cd "$pkgname"
  cargo build --release --locked
}

package() {
  cd "$pkgname"

  install -Dm755 "target/release/tcui" "$pkgdir/usr/bin/tcui"
  install -Dm644 "README.md" "$pkgdir/usr/share/doc/tcui/README.md"
  install -Dm644 "LICENSE" "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 \
    "assets/models/potion-base-8M/LICENSE" \
    "$pkgdir/usr/share/licenses/$pkgname/potion-base-8M-LICENSE"
}
