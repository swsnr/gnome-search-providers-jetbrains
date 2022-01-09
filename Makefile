DESTDIR =
PREFIX = /usr/local

## Files are installed into these three base directories.
# The path to install the service executable (gnome-search-providers-jetbrains)
LIBEXECDIR = $(PREFIX)/lib/gnome-search-providers-jetbrains

# The path to install systemd user units in
USERUNITDIR = $(PREFIX)/lib/systemd/user

# The base path for dbus services and gnome-shell search providers
DATADIR = $(PREFIX)/share
DBUS_SERVICES_DIR = $(DATADIR)/dbus-1/services
SEARCH_PROVIDERS_DIR = $(DATADIR)/gnome-shell/search-providers

SEARCH_PROVIDERS = $(wildcard providers/*.ini)

.PHONY: build
build:
	cargo build --release --locked

.PHONY: install
install: build
	mkdir -p target/dbus-1 target/systemd
	sed "s:{LIBEXECDIR}:$(LIBEXECDIR):g" "dbus-1/de.swsnr.searchprovider.Jetbrains.service" > "target/dbus-1/de.swsnr.searchprovider.Jetbrains.service"
	sed "s:{LIBEXECDIR}:$(LIBEXECDIR):g" "systemd/gnome-search-providers-jetbrains.service" > "target/systemd/gnome-search-providers-jetbrains.service"

	install -Dm644 -t $(DESTDIR)$(SEARCH_PROVIDERS_DIR) $(SEARCH_PROVIDERS)
	install -Dm755 -t $(DESTDIR)$(LIBEXECDIR) target/release/gnome-search-providers-jetbrains
	install -Dm644 -t $(DESTDIR)$(USERUNITDIR) target/systemd/gnome-search-providers-jetbrains.service
	install -Dm644 -t $(DESTDIR)$(DBUS_SERVICES_DIR) target/dbus-1/de.swsnr.searchprovider.Jetbrains.service

.PHONY: uninstall
uninstall:
	rm -f $(addprefix $(DESTDIR)$(SEARCH_PROVIDERS_DIR)/,$(notdir $(SEARCH_PROVIDERS)))
	rm -rf $(DESTDIR)$(LIBEXECDIR)/
	rm -f $(DESTDIR)$(USERUNITDIR)/gnome-search-providers-jetbrains.service
	rm -f $(DESTDIR)$(DBUS_SERVICES_DIR)/de.swsnr.searchprovider.Jetbrains.service

