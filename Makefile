DESTDIR =
PREFIX = /usr/local

SEARCH_PROVIDERS_DIR = $(DESTDIR)/$(PREFIX)/share/gnome-shell/search-providers
LIBDIR = $(DESTDIR)/$(PREFIX)/lib
DATADIR = $(DESTDIR)/$(PREFIX)/share

SEARCH_PROVIDERS = $(wildcard providers/*.ini)

.PHONY: build
build:
	cargo build --release

.PHONY: install
install:
	install -Dm644 -t $(SEARCH_PROVIDERS_DIR) $(SEARCH_PROVIDERS)
	install -Dm755 -t $(LIBDIR)/gnome-search-providers-jetbrains/ target/release/gnome-search-providers-jetbrains
	install -Dm644 -t $(LIBDIR)/systemd/user/ systemd/de.swsnr.searchprovider.Jetbrains.service
	install -Dm644 -t $(DATADIR)/dbus-1/services dbus-1/de.swsnr.searchprovider.Jetbrains.service

.PHONY: uninstall
uninstall:
	rm -f $(addprefix $(SEARCH_PROVIDERS_DIR)/,$(notdir $(SEARCH_PROVIDERS)))
	rm -rf $(LIBDIR)/gnome-search-providers-jetbrains/
	rm -f $(LIBDIR)/systemd/user/de.swsnr.searchprovider.Jetbrains.service
	rm -f $(DATADIR)/dbus-1/services/de.swsnr.searchprovider.Jetbrains.service
