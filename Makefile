DESTDIR =
PREFIX = /usr

SEARCH_PROVIDERS_DIR = $(DESTDIR)/$(PREFIX)/share/gnome-shell/search-providers

SEARCH_PROVIDERS = $(wildcard providers/*.ini)

.PHONY: install
install:
	install -Dm644 -t $(SEARCH_PROVIDERS_DIR) $(SEARCH_PROVIDERS)

.PHONY: uninstall
uninstall:
	rm -f $(addprefix $(SEARCH_PROVIDERS_DIR),$(basename $(SEARCH_PROVIDERS)))
