package humanize

import (
	"fmt"
)

func FileSize(nbytes int64) string {
	if nbytes < 1024 {
		return fmt.Sprintf("%d", nbytes)
	}
	var exp int
	n := float64(nbytes)
	for exp = 0; exp < 4; exp++ {
		n /= 1024
		if n < 1024 {
			break
		}
	}
	return fmt.Sprintf("%.1f%c", float64(n), "KMGT"[exp])
}
