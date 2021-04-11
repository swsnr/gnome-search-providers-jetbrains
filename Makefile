DESTDIR =
PREFIX = /usr

SEARCH_PROVIDERS_DIR = $(DESTDIR)/$(PREFIX)/share/gnome-shell/search-providers
LIBDIR = $(DESTDIR)/$(PREFIX)/lib
DATADIR = $(DESTDIR)/$(PREFIX)/share

SEARCH_PROVIDERS = $(wildcard providers/*.ini)

.PHONY: install
install:
	install -Dm644 -t $(SEARCH_PROVIDERS_DIR) $(SEARCH_PROVIDERS)
	install -Dm755 -t $(LIBDIR)/gnome-search-providers-jetbrains/ jetbrains-search-provider.py
	install -Dm644 -t $(LIBDIR)/systemd/user/ de.swsnr.searchprovider.Jetbrains.service
	install -Dm644 de.swsnr.searchprovider.Jetbrains.dbus $(DATADIR)/dbus-1/services/de.swsnr.searchprovider.Jetbrains.service

.PHONY: uninstall
uninstall:
	rm -f $(addprefix $(SEARCH_PROVIDERS_DIR),$(basename $(SEARCH_PROVIDERS)))
	rm -rf $(LIBDIR)/gnome-search-providers-jetbrains/
	rm -f $(LIBDIR)/systemd/user/de.swsnr.searchprovider.Jetbrains.service
	rm -f $(DATADIR)/dbus-1/services/de.swsnr.searchprovider.Jetbrains.service
