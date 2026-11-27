pkgname=pyroclear
_name=pyroclear
pkgver=r83.a13c6cf
pkgrel=1
pkgdesc="Clear your terminal with a crackling wall of ASCII flames."
arch=("x86_64" "aarch64")
url="https://github.com/shreyanth-sureshkrishnaa/pyroclear"
license=()
depends=()
makedepends=('cargo' 'git')
optdepends=()
source=("git+${url}")
options=("!debug")
sha256sums=("SKIP")

pkgver() {
  cd "$srcdir/$_name"
  printf "r%s.%s" "$(git rev-list --count HEAD)" "$(git rev-parse --short=7 HEAD)"
}
build() {
  cd "$srcdir/$_name"

  cargo build --release
}
package() {
  cd "$srcdir/$_name"

  # Binary
  install -Dm755 target/release/pyroclear "$pkgdir/usr/bin/pyroclear"
}
