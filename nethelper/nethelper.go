package main

import (
	"fmt"
	"net"
	"os"
	"os/exec"
	"strings"
	"strconv"
)

const socketPath = "/tmp/nethelper.sock"

var iface string

func run(name string, args ...string) {
	cmd := exec.Command(name, args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	_ = cmd.Run()
}

func detectIface() string {
	out, err := exec.Command("sh", "-c", "ip route | awk '/default/ {print $5; exit}'").Output()
	if err != nil {
		return "eth0"
	}
	return strings.TrimSpace(string(out))
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
    exec.Command("iptables", "-F").Run()
}

func blockAll() {
	run("iptables", "-I", "INPUT", "-j", "DROP")
	run("iptables", "-I", "OUTPUT", "-j", "DROP")
}

func lagAll(delay string, loss string) {
	run("tc", "qdisc", "replace", "dev", iface, "root",
		"netem", "delay", delay+"ms", "loss", loss+"%")
}

func parseFlags(parts []string) (inbound, outbound, udp, tcp bool) {
    inbound, outbound, udp, tcp = true, true, true, false
    for _, p := range parts {
        kv := strings.SplitN(p, "=", 2)
        if len(kv) != 2 { continue }
        v := kv[1] == "1"
        switch kv[0] {
        case "in":  inbound = v
        case "out": outbound = v
        case "udp": udp = v
        case "tcp": tcp = v
        }
    }
    return
}

func blockSelected(inbound, outbound, udp, tcp bool) {
    protos := []string{}
    if udp { protos = append(protos, "udp") }
    if tcp { protos = append(protos, "tcp") }
    if len(protos) == 0 { return }

    for _, proto := range protos {
        if inbound {
            run("iptables", "-I", "INPUT", "-p", proto, "-j", "DROP")
        }
        if outbound {
            run("iptables", "-I", "OUTPUT", "-p", proto, "-j", "DROP")
        }
    }
}

func lagSelected(delayMs int, inbound, outbound, udp, tcp bool) {
    delay := strconv.Itoa(delayMs)
    // tc netem only controls outbound on the interface
    // for inbound we use an IFB device
    if outbound {
        run("tc", "qdisc", "replace", "dev", iface, "root",
            "netem", "delay", delay+"ms")
    }
    if inbound {
        // redirect ingress to ifb0 and apply netem there
        run("ip", "link", "add", "ifb0", "type", "ifb")
        run("ip", "link", "set", "ifb0", "up")
        run("tc", "qdisc", "add", "dev", iface, "ingress")
        run("tc", "filter", "add", "dev", iface, "parent", "ffff:",
            "protocol", "ip", "u32", "match", "u32", "0", "0",
            "action", "mirred", "egress", "redirect", "dev", "ifb0")
        run("tc", "qdisc", "replace", "dev", "ifb0", "root",
            "netem", "delay", delay+"ms")
    }
}

func handle(conn net.Conn) {
    defer conn.Close()
    buf := make([]byte, 2048)
    n, _ := conn.Read(buf)
    cmd := strings.TrimSpace(string(buf[:n]))
    parts := strings.Fields(cmd)
    if len(parts) == 0 { return }

    switch parts[0] {
    case "ping":
        conn.Write([]byte("pong"))
    case "reset":
        resetAll()
    case "block":
        resetAll()
        in, out, udp, tcp := parseFlags(parts[1:])
        blockSelected(in, out, udp, tcp)
    case "lag":
        resetAll()
        in, out, udp, tcp := parseFlags(parts[1:])
        delay := 100
        for _, p := range parts[1:] {
            kv := strings.SplitN(p, "=", 2)
            if len(kv) == 2 && kv[0] == "delay" {
                delay, _ = strconv.Atoi(kv[1])
            }
        }
        lagSelected(delay, in, out, udp, tcp)
    }
}

func main() {
	exec.Command("modprobe", "ifb").Run()
        iface = detectIface()

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
