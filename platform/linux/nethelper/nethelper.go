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
	"github.com/vishvananda/netlink"
	"golang.org/x/sys/unix"
)

const socketPath = "/tmp/nethelper.sock"

var (
	iface      string
	nlHandle   *netlink.Handle
	ipt        *iptables.IPTables
	cmdMu      sync.Mutex
	inFlightMu sync.Mutex
	inFlight   = map[string]chan struct{}{}
)

func detectIface() string {
	routes, err := nlHandle.RouteList(nil, netlink.FAMILY_V4)
	if err != nil {
		return "eth0"
	}
	for _, r := range routes {
		if r.Dst == nil {
			link, err := nlHandle.LinkByIndex(r.LinkIndex)
			if err == nil {
				return link.Attrs().Name
			}
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

func ifb0Link() netlink.Link {
	l, _ := nlHandle.LinkByName("ifb0")
	return l
}

func delRootQdisc(link netlink.Link) {
	_ = nlHandle.QdiscDel(&netlink.GenericQdisc{
		QdiscAttrs: netlink.QdiscAttrs{
			LinkIndex: link.Attrs().Index,
			Handle:    netlink.MakeHandle(1, 0),
			Parent:    netlink.HANDLE_ROOT,
		},
		QdiscType: "noqueue",
	})
}

func resetAll() {
	link, err := nlHandle.LinkByName(iface)
	if err == nil {
		delRootQdisc(link)
		_ = nlHandle.QdiscDel(&netlink.Ingress{
			QdiscAttrs: netlink.QdiscAttrs{
				LinkIndex: link.Attrs().Index,
				Handle:    netlink.MakeHandle(0xffff, 0),
				Parent:    netlink.HANDLE_INGRESS,
			},
		})
	}

	if ifb := ifb0Link(); ifb != nil {
		delRootQdisc(ifb)
		_ = nlHandle.LinkSetDown(ifb)
		_ = nlHandle.LinkDel(ifb)
	}

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

func netemQdisc(linkIndex int, parent uint32, delayMs int) *netlink.Netem {
	return &netlink.Netem{
		QdiscAttrs: netlink.QdiscAttrs{
			LinkIndex: linkIndex,
			Handle:    netlink.MakeHandle(1, 0),
			Parent:    parent,
		},
		Latency: uint32(delayMs) * 1000,
	}
}

func lagSelected(delayMs int, inbound, outbound bool) {
	link, err := nlHandle.LinkByName(iface)
	if err != nil {
		return
	}

	var wg sync.WaitGroup

	if outbound {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_ = nlHandle.QdiscReplace(netemQdisc(link.Attrs().Index, netlink.HANDLE_ROOT, delayMs))
		}()
	}

	if inbound {
		wg.Add(1)
		go func() {
			defer wg.Done()

			ifb := &netlink.Ifb{LinkAttrs: netlink.LinkAttrs{Name: "ifb0"}}
			_ = nlHandle.LinkAdd(ifb)
			ifbLink, err := nlHandle.LinkByName("ifb0")
			if err != nil {
				return
			}
			_ = nlHandle.LinkSetUp(ifbLink)

			_ = nlHandle.QdiscAdd(&netlink.Ingress{
				QdiscAttrs: netlink.QdiscAttrs{
					LinkIndex: link.Attrs().Index,
					Handle:    netlink.MakeHandle(0xffff, 0),
					Parent:    netlink.HANDLE_INGRESS,
				},
			})

			_ = nlHandle.FilterAdd(&netlink.U32{
				FilterAttrs: netlink.FilterAttrs{
					LinkIndex: link.Attrs().Index,
					Parent:    netlink.MakeHandle(0xffff, 0),
					Priority:  1,
					Protocol:  unix.ETH_P_IP,
				},
				Actions: []netlink.Action{
					&netlink.MirredAction{
						ActionAttrs:  netlink.ActionAttrs{Action: netlink.TC_ACT_STOLEN},
						MirredAction: netlink.TCA_EGRESS_REDIR,
						Ifindex:      ifbLink.Attrs().Index,
					},
				},
			})

			_ = nlHandle.QdiscReplace(netemQdisc(ifbLink.Attrs().Index, netlink.HANDLE_ROOT, delayMs))
		}()
	}

	wg.Wait()
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

	nlHandle, err = netlink.NewHandle(unix.NETLINK_ROUTE)
	if err != nil {
		panic(fmt.Sprintf("netlink: %v", err))
	}
	defer nlHandle.Close()

	ipt, err = iptables.New()
	if err != nil {
		panic(fmt.Sprintf("iptables: %v", err))
	}

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
