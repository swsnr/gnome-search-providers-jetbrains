DESTDIR =
PREFIX = /usr/local

SEARCH_PROVIDERS_DIR = $(DESTDIR)/$(PREFIX)/share/gnome-shell/search-providers
LIBDIR = $(DESTDIR)/$(PREFIX)/lib
DATADIR = $(DESTDIR)/$(PREFIX)/share

SEARCH_PROVIDERS = $(wildcard providers/*.ini)

.PHONY: build
build:
	cargo build --release --locked
	mkdir -p target/dbus-1 target/systemd
	sed "s:{PREFIX}:$(PREFIX):g" "dbus-1/de.swsnr.searchprovider.Jetbrains.service" > "target/dbus-1/de.swsnr.searchprovider.Jetbrains.service"
	sed "s:{PREFIX}:$(PREFIX):g" "systemd/de.swsnr.searchprovider.Jetbrains.service" > "target/systemd/de.swsnr.searchprovider.Jetbrains.service"


.PHONY: install
install:
	install -Dm644 -t $(SEARCH_PROVIDERS_DIR) $(SEARCH_PROVIDERS)
	install -Dm755 -t $(LIBDIR)/gnome-search-providers-jetbrains/ target/release/gnome-search-providers-jetbrains
	install -Dm644 -t $(LIBDIR)/systemd/user/ target/systemd/de.swsnr.searchprovider.Jetbrains.service
	install -Dm644 -t $(DATADIR)/dbus-1/services target/dbus-1/de.swsnr.searchprovider.Jetbrains.service

.PHONY: uninstall
uninstall:
	rm -f $(addprefix $(SEARCH_PROVIDERS_DIR)/,$(notdir $(SEARCH_PROVIDERS)))
	rm -rf $(LIBDIR)/gnome-search-providers-jetbrains/
	rm -f $(LIBDIR)/systemd/user/de.swsnr.searchprovider.Jetbrains.service
	rm -f $(DATADIR)/dbus-1/services/de.swsnr.searchprovider.Jetbrains.service
