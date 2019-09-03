build: main.go
	go build -o srv main.go

clean:
	rm -f srv
