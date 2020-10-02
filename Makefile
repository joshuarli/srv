NAME := srv
VERSION := $(shell printf git-%s-%s\\n "$$(git describe --tags --abbrev=0)" "$$(git log -1 --pretty=format:'%h')")
GO_BUILDFLAGS := -trimpath
GO_LDFLAGS := -ldflags "-s -w -X main.VERSION=$(VERSION)"
GO_LDFLAGS_DEBUG := -ldflags "-X main.VERSION=$(VERSION)-DEBUG"
GO_LDFLAGS_STATIC := -tags netgo -ldflags "-s -w -X main.VERSION=$(VERSION) -extldflags -static"

.PHONY: build debug fmt lint clean release

build: clean fmt lint $(NAME)

$(NAME): main.go
	go build $(GO_BUILDFLAGS) -o $@ $(GO_LDFLAGS) .

debug: $(NAME)-debug
$(NAME)-debug: main.go
	go build $(GO_BUILDFLAGS) -o $@ -gcflags="all=-N -l" $(GO_LDFLAGS_DEBUG) .

fmt:
	go fmt

lint:
	golint

clean:
	rm -f $(NAME) $(NAME)-debug
	rm -rf release

# release static crossbuilds
define buildrelease
GOOS=$(1) GOARCH=$(2) go build $(GO_BUILDFLAGS) \
	 -a \
	 -o release/$(NAME)-$(1)-$(2) \
	 $(GO_LDFLAGS_STATIC) . ;
upx -9 release/$(NAME)-$(1)-$(2);
sha512sum release/$(NAME)-$(1)-$(2) > release/$(NAME)-$(1)-$(2).sha512sum;
endef

GOOSARCHES = linux/arm linux/arm64 linux/amd64 darwin/amd64

release: main.go
	$(foreach GOOSARCH,$(GOOSARCHES), $(call buildrelease,$(subst /,,$(dir $(GOOSARCH))),$(notdir $(GOOSARCH))))
