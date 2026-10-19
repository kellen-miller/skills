package main

import (
	"context"
	"os"
	"os/signal"
	"syscall"

	"github.com/kellen-miller/skills/internal/skillctl"
)

func main() {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	code := skillctl.Run(ctx, os.Args[1:], os.Stdout, os.Stderr)
	stop()
	os.Exit(code)
}
