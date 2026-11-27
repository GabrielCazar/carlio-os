# Stages a trusted host ELF and its loader and shared libraries in the test root.
install_elf() {
  local root=$1 binary=$2 guest_path=$3 interpreter dependencies library arrow resolved
  install -D -m 0755 "$binary" "$root$guest_path"
  interpreter=$(LC_ALL=C readelf -l "$binary" | sed -n 's/.*Requesting program interpreter: \(\/[^]]*\)].*/\1/p')
  if [[ -n $interpreter ]]; then
    install -D -m 0755 "$interpreter" "$root$interpreter"
  fi
  dependencies=$(ldd "$binary")
  while read -r library arrow resolved _; do
    if [[ $arrow == '=>' ]]; then
      [[ $resolved == /* ]] || { echo "Dependência ausente: $library" >&2; return 1; }
      install -D -m 0644 "$resolved" "$root${resolved%/*}/$library"
    elif [[ $library == /* ]]; then
      install -D -m 0755 "$library" "$root$library"
    fi
  done <<< "$dependencies"
}
