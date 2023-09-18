DESTDIR =
PREFIX = /usr/local

## Files are installed into these three base directories.
# The path to install the service executable (gnome-search-providers-jetbrains)
BINDIR = $(PREFIX)/bin
# The path to install systemd user units in
USERUNITDIR = $(PREFIX)/lib/systemd/user
# The base path for dbus services, gnome-shell search providers and gschema
DATADIR = $(PREFIX)/share
DBUS_SERVICES_DIR = $(DATADIR)/dbus-1/services
SEARCH_PROVIDERS_DIR = $(DATADIR)/gnome-shell/search-providers
GSCHEMA_DIR = $(DATADIR)/glib-2.0/schemas

SEARCH_PROVIDERS = $(wildcard providers/*.ini)

.PHONY: build
build:
	cargo build --release --locked

.PHONY: install
install:
	install -Dm644 -t $(DESTDIR)$(SEARCH_PROVIDERS_DIR) $(SEARCH_PROVIDERS)
	install -Dm644 -t $(DESTDIR)$(USERUNITDIR) systemd/gnome-search-providers-jetbrains.service
	install -Dm644 -t $(DESTDIR)$(DBUS_SERVICES_DIR) dbus-1/de.swsnr.searchprovider.Jetbrains.service
	install -Dm755 -t $(DESTDIR)$(BINDIR) target/release/gnome-search-providers-jetbrains
	install -Dm644 -t $(DESTDIR)$(GSCHEMA_DIR) gsettings/de.swsnr.searchprovider.jetbrains.gschema.xml

.PHONY: schema-compile
schema-compile:
	glib-compile-schemas --strict $(DESTDIR)$(GSCHEMA_DIR)

.PHONY: uninstall
uninstall:
	rm -f $(addprefix $(DESTDIR)$(SEARCH_PROVIDERS_DIR)/,$(notdir $(SEARCH_PROVIDERS)))
	rm -rf $(DESTDIR)$(BINDIR)/
	rm -f $(DESTDIR)$(USERUNITDIR)/gnome-search-providers-jetbrains.service
	rm -f $(DESTDIR)$(DBUS_SERVICES_DIR)/de.swsnr.searchprovider.Jetbrains.service
	rm -f $(DESTDIR)$(GSCHEMA_DIR) gsettings/de.swsnr.searchprovider.jetbrains.gschema.xml

