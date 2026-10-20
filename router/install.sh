#!/bin/sh
# Corepass 0.3.0 installer: OpenWrt 24.10 / arm64. Does not switch network traffic.
set -eu
umask 077
VERSION=0.3.0
BASE="https://github.com/orangepony4/corepass/releases/download/v$VERSION"
die() { printf '\nCorepass: %s\n' "$*" >&2; exit 1; }
say() { printf 'Corepass: %s\n' "$*"; }
[ "$(id -u)" = 0 ] || die 'Запустите установщик от root на роутере.'
[ -r /etc/openwrt_release ] || die 'Нужен OpenWrt 24.10.'
. /etc/openwrt_release
case "${DISTRIB_RELEASE:-}" in 24.10*) ;; *) die 'Этот релиз проверяет совместимость только с OpenWrt 24.10. Прошивка не изменялась.';; esac
[ "$(uname -m)" = aarch64 ] || die 'Этот пакет предназначен для ARM64 / aarch64.'
case "${DISTRIB_TARGET:-}" in mediatek/filogic) ;; *) die 'Первый релиз ограничен платформой mediatek/filogic.';; esac
if [ -r /usr/lib/corepass/version ]; then
    [ "$(cat /usr/lib/corepass/version)" = "$VERSION" ] && { say 'Эта версия уже установлена. Откройте LuCI → Службы → Corepass.'; exit 0; }
    die 'Обнаружена другая версия Corepass. Обновление действующей установки требует отдельной миграции; ничего не изменено.'
fi
command -v uclient-fetch >/dev/null || die 'Не найден штатный uclient-fetch.'
command -v sha256sum >/dev/null || die 'Не найден sha256sum.'
command -v opkg >/dev/null || die 'Не найден opkg. Требуется стандартный OpenWrt 24.10.'
stage="$(mktemp -d /tmp/corepass-install.XXXXXX)"
cleanup() { case "$stage" in /tmp/corepass-install.??????) rm -rf -- "$stage";; esac; }
trap cleanup EXIT INT TERM
fetch() { uclient-fetch -q -T 45 -O "$stage/$1" "$BASE/$1" || die "Не удалось загрузить $1. Ничего не включалось."; }
say 'Скачиваем проверяемые файлы релиза…'
fetch SHA256SUMS
fetch corepass-openwrt-arm64.squashfs
fetch corepass-openwrt-ui.tar.gz
for asset in corepass-openwrt-arm64.squashfs corepass-openwrt-ui.tar.gz; do
    [ "$(grep -Ec "^[0-9a-f]{64}  $asset\$" "$stage/SHA256SUMS")" = 1 ] || die "Нет однозначной контрольной суммы $asset."
done
grep -E '^[0-9a-f]{64}  corepass-openwrt-(arm64.squashfs|ui.tar.gz)$' "$stage/SHA256SUMS" > "$stage/router-sums"
(cd "$stage" && sha256sum -c router-sums) || die 'Контрольные суммы не совпадают. Установка остановлена.'
image_kib="$(du -k "$stage/corepass-openwrt-arm64.squashfs" | awk '{print $1}')"
free_kib="$(df -Pk /overlay | awk 'NR==2 {print $4}')"
[ -n "$free_kib" ] || die 'Не удалось определить свободное место на /overlay.'
[ "$free_kib" -ge "$((image_kib + 4096))" ] || die "Не хватает флеш-памяти: нужно минимум $((image_kib + 4096)) КБ, доступно $free_kib КБ."
# Only exact owned paths may be extracted; no firmware, network config, or other manager files.
tar -tzf "$stage/corepass-openwrt-ui.tar.gz" > "$stage/files" || die 'Повреждён архив интерфейса.'
while IFS= read -r entry; do
    case "$entry" in
      etc/init.d/corepass|usr/libexec/corepass/mount-engine|usr/libexec/rpcd/corepass|usr/share/rpcd/acl.d/luci-app-corepass.json|usr/share/luci/menu.d/luci-app-corepass.json|www/luci-static/resources/view/corepass.js|lib/upgrade/keep.d/corepass|usr/share/corepass/LICENSE|usr/share/corepass/THIRD-PARTY.md) ;;
      *) die "Неожиданный путь в архиве: $entry";;
    esac
done < "$stage/files"
tar -tvzf "$stage/corepass-openwrt-ui.tar.gz" | awk 'substr($1,1,1)!="-" { bad=1 } END { exit bad }' || die 'Ссылки и специальные файлы в архиве запрещены.'
say 'Устанавливаем необходимые пакеты OpenWrt…'
opkg update || die 'Не удалось обновить список пакетов. Настройки сети не менялись.'
opkg install kmod-loop kmod-tun kmod-nft-queue ca-bundle luci-base rpcd || die 'Не удалось установить зависимости. Не используйте --force-depends: проверьте соответствие ядра и репозитория прошивки.'
grep -qw squashfs /proc/filesystems || die 'Ядро не поддерживает SquashFS.'
mkdir -p /usr/lib/corepass/runtime /etc/corepass
chmod 700 /etc/corepass
cp "$stage/corepass-openwrt-arm64.squashfs" /usr/lib/corepass/engine.squashfs.new
mv /usr/lib/corepass/engine.squashfs.new /usr/lib/corepass/engine.squashfs
tar -xzf "$stage/corepass-openwrt-ui.tar.gz" -C /
chmod 755 /etc/init.d/corepass /usr/libexec/rpcd/corepass /usr/libexec/corepass/mount-engine
[ -e /etc/corepass/settings.json ] || printf '%s\n' '{"enabled":false,"subscription":"","devices":[],"lan_interface":"br-lan"}' > /etc/corepass/settings.json
/usr/libexec/corepass/mount-engine || die 'Не удалось смонтировать образ движка. Сеть не переключалась.'
/usr/lib/corepass/runtime/corepass-router list >/dev/null || die 'Не удалось запустить управляющий модуль. Сеть не переключалась.'
printf '%s\n' "$VERSION" > /usr/lib/corepass/version
/etc/init.d/corepass disable
# Reload only the RPC plugin registry, never network/firewall/DNS or other VPN services.
/etc/init.d/rpcd restart
say 'Готово. LuCI → Службы → Corepass.'
say 'Corepass выключен. Добавьте HTTPS-подписку и выберите одно тестовое устройство.'
say 'Steer, WARP, AmneziaWG, DNS и настройки Zapret Manager не изменялись.'
