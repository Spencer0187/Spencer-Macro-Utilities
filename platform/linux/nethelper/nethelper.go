package main

import (
	"fmt"
	"net"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"sync"

	"github.com/coreos/go-iptables/iptables"
)

const socketPath = "/tmp/nethelper.sock"

var (
	iface      string
	ipt        *iptables.IPTables
	cmdMu      sync.Mutex
	inFlightMu sync.Mutex
	inFlight   = map[string]chan struct{}{}
)

func detectIface() string {
	out, err := exec.Command("ip", "-4", "route", "show", "default").Output()
	if err != nil {
		return "eth0"
	}
	// "default via x.x.x.x dev ethX ..."
	fields := strings.Fields(string(out))
	for i, f := range fields {
		if f == "dev" && i+1 < len(fields) {
			return fields[i+1]
		}
	}
	return "eth0"
}

func setupSocket() net.Listener {
	_ = os.Remove(socketPath)
	l, err := net.Listen("unix", socketPath)
	if err != nil {
		panic(err)
	}
	_ = os.Chmod(socketPath, 0666)
	return l
}

func resetAll() {
	exec.Command("tc", "qdisc", "del", "dev", iface, "root").Run()
	exec.Command("tc", "qdisc", "del", "dev", iface, "ingress").Run()
	exec.Command("tc", "qdisc", "del", "dev", "ifb0", "root").Run()
	exec.Command("ip", "link", "set", "ifb0", "down").Run()
	exec.Command("ip", "link", "del", "ifb0").Run()

	_ = ipt.ClearChain("filter", "INPUT")
	_ = ipt.ClearChain("filter", "OUTPUT")
}

func parseFlags(parts []string) (inbound, outbound, udp, tcp bool) {
	inbound, outbound, udp, tcp = true, true, true, false
	for _, p := range parts {
		kv := strings.SplitN(p, "=", 2)
		if len(kv) != 2 {
			continue
		}
		v := kv[1] == "1"
		switch kv[0] {
		case "in":
			inbound = v
		case "out":
			outbound = v
		case "udp":
			udp = v
		case "tcp":
			tcp = v
		}
	}
	return
}

func blockSelected(inbound, outbound, udp, tcp bool) {
	protos := []string{}
	if udp {
		protos = append(protos, "udp")
	}
	if tcp {
		protos = append(protos, "tcp")
	}
	for _, proto := range protos {
		if inbound {
			_ = ipt.Insert("filter", "INPUT", 1, "-p", proto, "-j", "DROP")
		}
		if outbound {
			_ = ipt.Insert("filter", "OUTPUT", 1, "-p", proto, "-j", "DROP")
		}
	}
}

func lagSelected(delayMs int, inbound, outbound bool) {
	delay := fmt.Sprintf("%dms", delayMs)

	if outbound {
		exec.Command("tc", "qdisc", "replace", "dev", iface, "root", "netem", "delay", delay).Run()
	}

	if inbound {
		exec.Command("modprobe", "ifb").Run()
		exec.Command("ip", "link", "add", "ifb0", "type", "ifb").Run()
		exec.Command("ip", "link", "set", "ifb0", "up").Run()
		exec.Command("tc", "qdisc", "add", "dev", iface, "ingress").Run()
		exec.Command("tc", "filter", "add", "dev", iface, "parent", "ffff:", "protocol", "ip", "u32", "match", "u32", "0", "0", "action", "mirred", "egress", "redirect", "dev", "ifb0").Run()
		exec.Command("tc", "qdisc", "replace", "dev", "ifb0", "root", "netem", "delay", delay).Run()
	}
}

func dedup(key string, fn func()) {
	inFlightMu.Lock()
	if ch, ok := inFlight[key]; ok {
		inFlightMu.Unlock()
		<-ch
		return
	}
	ch := make(chan struct{})
	inFlight[key] = ch
	inFlightMu.Unlock()

	fn()

	inFlightMu.Lock()
	delete(inFlight, key)
	inFlightMu.Unlock()
	close(ch)
}

func handle(conn net.Conn) {
	defer conn.Close()

	buf := [256]byte{}
	n, _ := conn.Read(buf[:])
	cmd := strings.TrimSpace(string(buf[:n]))
	parts := strings.Fields(cmd)
	if len(parts) == 0 {
		return
	}

	switch parts[0] {
	case "ping":
		conn.Write([]byte("pong"))

	case "reset":
		dedup("reset", func() {
			cmdMu.Lock()
			defer cmdMu.Unlock()
			resetAll()
		})

	case "block":
		in, out, udp, tcp := parseFlags(parts[1:])
		key := fmt.Sprintf("block in=%v out=%v udp=%v tcp=%v", in, out, udp, tcp)
		dedup(key, func() {
			cmdMu.Lock()
			defer cmdMu.Unlock()
			resetAll()
			blockSelected(in, out, udp, tcp)
		})

	case "lag":
		in, out, _, _ := parseFlags(parts[1:])
		delay := 100
		for _, p := range parts[1:] {
			kv := strings.SplitN(p, "=", 2)
			if len(kv) == 2 && kv[0] == "delay" {
				delay, _ = strconv.Atoi(kv[1])
			}
		}
		key := fmt.Sprintf("lag delay=%d in=%v out=%v", delay, in, out)
		dedup(key, func() {
			cmdMu.Lock()
			defer cmdMu.Unlock()
			resetAll()
			lagSelected(delay, in, out)
		})
	}
}

func main() {
	var err error

	ipt, err = iptables.New()
	if err != nil {
		panic(fmt.Sprintf("iptables: %v", err))
	}

	exec.Command("modprobe", "ifb").Run()
	iface = detectIface()
	fmt.Printf("[daemon] iface: %s\n", iface)

	l := setupSocket()
	defer l.Close()

	fmt.Println("[daemon] running on", socketPath)

	for {
		conn, err := l.Accept()
		if err != nil {
			continue
		}
		go handle(conn)
	}
}
