package main

import (
	"bufio"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"os/signal"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/coreos/go-iptables/iptables"
)

const (
	maxCommandBytes = 8192
	maxTargets      = 64
	robloxIPv4CIDR  = "128.116.0.0/16"
	ownerPollPeriod = 250 * time.Millisecond
)

type firewall interface {
	Append(table, chain string, rulespec ...string) error
	ChainExists(table, chain string) (bool, error)
	ClearAndDeleteChain(table, chain string) error
	Delete(table, chain string, rulespec ...string) error
	Exists(table, chain string, rulespec ...string) (bool, error)
	InsertUnique(table, chain string, pos int, rulespec ...string) error
	NewChain(table, chain string) error
}

type targetMode int

const (
	targetAll targetMode = iota
	targetRoblox
	targetCustom
)

type blockConfig struct {
	inbound     bool
	outbound    bool
	udp         bool
	tcp         bool
	mode        targetMode
	remoteIPs   []string
	remotePorts []int
}

type lagConfig struct {
	blockConfig
	delayMs int
}

type ruleSpec []string

type ownerIdentity struct {
	pid       int32
	uid       uint32
	startTime uint64
}

type ownerSnapshot struct {
	realUID      uint32
	effectiveUID uint32
	startTime    uint64
	state        byte
}

func joinErrors(values ...error) error {
	messages := make([]string, 0, len(values))
	var first error
	for _, value := range values {
		if value != nil {
			if first == nil {
				first = value
			}
			messages = append(messages, value.Error())
		}
	}
	switch len(messages) {
	case 0:
		return nil
	case 1:
		return first
	}
	return errors.New(strings.Join(messages, "\n"))
}

type nethelper struct {
	owner       ownerIdentity
	socketPath  string
	inputChain  string
	outputChain string
	firewall    firewall
	traffic     *trafficController
}

// commandRunner makes the traffic-control protocol testable without granting
// the unit-test process CAP_NET_ADMIN.
type commandRunner interface {
	run(name string, args ...string) (string, error)
}

type systemCommandRunner struct{}

func (systemCommandRunner) run(name string, args ...string) (string, error) {
	output, err := exec.Command(name, args...).CombinedOutput()
	return string(output), err
}

// trafficController owns only the qdiscs, IFB device, and filter priority it
// creates.  In particular it never uses `tc qdisc del ... root` on a qdisc it
// did not install.  Outbound shaping is permitted only over an untouched,
// default fq_codel root qdisc, which we can restore exactly.
type trafficController struct {
	uid           uint32
	runner        commandRunner
	interfaceName string
	ifbName       string
	inbound       bool
	outbound      bool
	createdClsact bool
}

const (
	smuTrafficFilterPriority = "49152"
	smuTrafficRootHandle     = "1:"
	smuTrafficDelayHandle    = "30:"
)

func newTrafficController(uid uint32, runner commandRunner) *trafficController {
	return &trafficController{
		uid:     uid,
		runner:  runner,
		ifbName: fmt.Sprintf("smuifb%d", uid),
	}
}

func (t *trafficController) command(name string, args ...string) error {
	output, err := t.runner.run(name, args...)
	if err != nil {
		return fmt.Errorf("%s %s: %w (%s)", name, strings.Join(args, " "), err, strings.TrimSpace(output))
	}
	return nil
}

func (t *trafficController) defaultInterface() (string, error) {
	output, err := t.runner.run("ip", "-4", "route", "show", "default")
	if err != nil {
		return "", fmt.Errorf("find default IPv4 route: %w", err)
	}
	fields := strings.Fields(output)
	for index, field := range fields {
		if field == "dev" && index+1 < len(fields) {
			return fields[index+1], nil
		}
	}
	return "", errors.New("no default IPv4 interface was found")
}

func isDefaultFqCodelQdisc(output string) bool {
	lines := strings.Split(strings.TrimSpace(output), "\n")
	if len(lines) != 1 {
		return false
	}
	line := strings.TrimSpace(lines[0])
	return strings.HasPrefix(line, "qdisc fq_codel ") &&
		strings.Contains(line, " root ") &&
		strings.Contains(line, "limit 10240p flows 1024 quantum 1514 target 5ms interval 100ms") &&
		strings.Contains(line, "memory_limit 32Mb ecn drop_batch 64")
}

func (t *trafficController) ensureOutboundRoot(delayMs int) error {
	output, err := t.runner.run("tc", "qdisc", "show", "dev", t.interfaceName)
	if err != nil {
		return fmt.Errorf("inspect %s qdisc: %w", t.interfaceName, err)
	}
	if !isDefaultFqCodelQdisc(output) {
		return errors.New("outbound fake lag is unavailable because this interface has custom traffic-control settings; SMU will not replace them")
	}
	if err := t.command("tc", "qdisc", "replace", "dev", t.interfaceName, "root", "handle", smuTrafficRootHandle, "prio", "bands", "3"); err != nil {
		return err
	}
	t.outbound = true
	if err := t.command("tc", "qdisc", "replace", "dev", t.interfaceName, "parent", "1:3", "handle", smuTrafficDelayHandle, "netem", "delay", strconv.Itoa(delayMs)+"ms", "limit", "1000"); err != nil {
		_ = t.restoreOutboundRoot()
		return err
	}
	return nil
}

func (t *trafficController) ensureInboundIfb(delayMs int) error {
	filters, filterErr := t.runner.run("tc", "filter", "show", "dev", t.interfaceName, "ingress")
	if filterErr == nil && strings.Contains(filters, "pref "+smuTrafficFilterPriority+" ") {
		return errors.New("inbound fake lag is unavailable because another filter uses SMU's reserved traffic-control priority")
	}
	// IFB is often built into the kernel.  When it is modular, ask kmod to
	// load it, but let ip link be the authoritative availability check.
	_, _ = t.runner.run("modprobe", "ifb")
	if err := t.command("ip", "link", "add", t.ifbName, "type", "ifb"); err != nil {
		return err
	}
	if err := t.command("ip", "link", "set", t.ifbName, "up"); err != nil {
		_ = t.command("ip", "link", "del", t.ifbName)
		return err
	}
	if err := t.command("tc", "qdisc", "replace", "dev", t.ifbName, "root", "handle", smuTrafficDelayHandle, "netem", "delay", strconv.Itoa(delayMs)+"ms", "limit", "1000"); err != nil {
		_ = t.command("ip", "link", "del", t.ifbName)
		return err
	}
	if err := t.command("tc", "qdisc", "add", "dev", t.interfaceName, "clsact"); err != nil {
		// clsact can already belong to another network component.  In that
		// case its filters remain intact and we only remove our priority.
		output, inspectErr := t.runner.run("tc", "qdisc", "show", "dev", t.interfaceName)
		if inspectErr != nil || !strings.Contains(output, "qdisc clsact") {
			_ = t.command("ip", "link", "del", t.ifbName)
			return err
		}
	} else {
		t.createdClsact = true
	}
	t.inbound = true
	return nil
}

func trafficFilterMatches(cfg blockConfig, inbound bool) [][]string {
	if cfg.mode == targetAll {
		return [][]string{{"matchall"}}
	}

	addressKey := "dst_ip"
	portKey := "dst_port"
	if inbound {
		addressKey = "src_ip"
		portKey = "src_port"
	}
	protocols := make([]string, 0, 2)
	if cfg.udp {
		protocols = append(protocols, "udp")
	}
	if cfg.tcp {
		protocols = append(protocols, "tcp")
	}

	addresses := cfg.remoteIPs
	if cfg.mode == targetRoblox {
		addresses = append([]string{robloxIPv4CIDR}, addresses...)
	}

	filters := make([][]string, 0, len(protocols)*(len(addresses)+len(cfg.remotePorts)))
	for _, protocol := range protocols {
		for _, address := range addresses {
			filters = append(filters, []string{"flower", "ip_proto", protocol, addressKey, address})
		}
		for _, port := range cfg.remotePorts {
			filters = append(filters, []string{"flower", "ip_proto", protocol, portKey, strconv.Itoa(port)})
		}
	}
	return filters
}

func (t *trafficController) addOutboundFilters(cfg blockConfig) error {
	for index, match := range trafficFilterMatches(cfg, false) {
		args := []string{"filter", "add", "dev", t.interfaceName, "parent", "1:", "protocol", "ip", "pref", smuTrafficFilterPriority, "handle", strconv.Itoa(index + 1)}
		args = append(args, match...)
		args = append(args, "flowid", "1:3")
		if err := t.command("tc", args...); err != nil {
			return err
		}
	}
	return nil
}

func (t *trafficController) addInboundFilters(cfg blockConfig) error {
	for index, match := range trafficFilterMatches(cfg, true) {
		args := []string{"filter", "add", "dev", t.interfaceName, "ingress", "protocol", "ip", "pref", smuTrafficFilterPriority, "handle", strconv.Itoa(index + 1)}
		args = append(args, match...)
		args = append(args, "action", "mirred", "egress", "redirect", "dev", t.ifbName)
		if err := t.command("tc", args...); err != nil {
			return err
		}
	}
	return nil
}

func (t *trafficController) applyLag(cfg lagConfig) error {
	if err := t.reset(); err != nil {
		return err
	}
	interfaceName, err := t.defaultInterface()
	if err != nil {
		return err
	}
	t.interfaceName = interfaceName
	if cfg.outbound {
		if err := t.ensureOutboundRoot(cfg.delayMs); err != nil {
			return err
		}
		if err := t.addOutboundFilters(cfg.blockConfig); err != nil {
			return joinErrors(err, t.reset())
		}
	}
	if cfg.inbound {
		if err := t.ensureInboundIfb(cfg.delayMs); err != nil {
			return joinErrors(err, t.reset())
		}
		if err := t.addInboundFilters(cfg.blockConfig); err != nil {
			return joinErrors(err, t.reset())
		}
	}
	return nil
}

func (t *trafficController) clearInboundIfb() error {
	if !t.inbound {
		return nil
	}
	var results []error
	if err := t.command("tc", "filter", "del", "dev", t.interfaceName, "ingress", "pref", smuTrafficFilterPriority); err != nil {
		results = append(results, err)
	}
	if t.createdClsact {
		if err := t.command("tc", "qdisc", "del", "dev", t.interfaceName, "clsact"); err != nil {
			results = append(results, err)
		}
	}
	if err := t.command("ip", "link", "del", t.ifbName); err != nil {
		results = append(results, err)
	}
	if len(results) == 0 {
		t.inbound = false
		t.createdClsact = false
		return nil
	}
	return joinErrors(results...)
}

func (t *trafficController) reset() error {
	return joinErrors(t.clearInboundIfb(), t.restoreOutboundRoot())
}

// cleanupStale removes only names and handles reserved by this helper.  It is
// the recovery path after an ungraceful helper termination; normal owner-exit
// cleanup goes through reset().
func (t *trafficController) cleanupStale() error {
	interfaceName, err := t.defaultInterface()
	if err != nil {
		return err
	}
	t.interfaceName = interfaceName
	var results []error
	output, inspectErr := t.runner.run("tc", "qdisc", "show", "dev", t.interfaceName)
	if inspectErr != nil {
		results = append(results, fmt.Errorf("inspect stale qdisc: %w", inspectErr))
	} else if strings.Contains(output, "qdisc prio 1: root") {
		if err := t.command("tc", "qdisc", "replace", "dev", t.interfaceName, "root", "fq_codel"); err != nil {
			results = append(results, err)
		}
	}
	if err := t.command("tc", "filter", "del", "dev", t.interfaceName, "ingress", "pref", smuTrafficFilterPriority); err != nil {
		// The filter is absent in the common case.  Its absence is harmless.
		if output, showErr := t.runner.run("tc", "filter", "show", "dev", t.interfaceName, "ingress"); showErr == nil && strings.Contains(output, "pref "+smuTrafficFilterPriority+" ") {
			results = append(results, err)
		}
	}
	if err := t.command("ip", "link", "del", t.ifbName); err != nil {
		if _, inspectErr := t.runner.run("ip", "link", "show", "dev", t.ifbName); inspectErr == nil {
			results = append(results, err)
		}
	}
	return joinErrors(results...)
}

func (t *trafficController) restoreOutboundRoot() error {
	if !t.outbound {
		return nil
	}
	err := t.command("tc", "qdisc", "replace", "dev", t.interfaceName, "root", "fq_codel")
	if err == nil {
		t.outbound = false
	}
	return err
}

func socketPathForUID(uid uint32) string {
	return fmt.Sprintf("/tmp/smu-nethelper-%d.sock", uid)
}

func chainNamesForUID(uid uint32) (string, string) {
	return fmt.Sprintf("SMU_IN_%d", uid), fmt.Sprintf("SMU_OUT_%d", uid)
}

func newNethelper(owner ownerIdentity, fw firewall) *nethelper {
	inputChain, outputChain := chainNamesForUID(owner.uid)
	return &nethelper{
		owner:       owner,
		socketPath:  socketPathForUID(owner.uid),
		inputChain:  inputChain,
		outputChain: outputChain,
		firewall:    fw,
		traffic:     newTrafficController(owner.uid, systemCommandRunner{}),
	}
}

func parseUID(value string) (uint32, error) {
	parsed, err := strconv.ParseUint(value, 10, 32)
	if err != nil {
		return 0, fmt.Errorf("invalid --uid: %w", err)
	}
	return uint32(parsed), nil
}

func parseOwnerPID(value string) (int32, error) {
	parsed, err := strconv.ParseInt(value, 10, 32)
	if err != nil || parsed <= 0 {
		if err == nil {
			err = errors.New("PID must be positive")
		}
		return 0, fmt.Errorf("invalid --owner-pid: %w", err)
	}
	return int32(parsed), nil
}

func parseOwnerStartTime(value string) (uint64, error) {
	parsed, err := strconv.ParseUint(value, 10, 64)
	if err != nil || parsed == 0 {
		if err == nil {
			err = errors.New("start time must be positive")
		}
		return 0, fmt.Errorf("invalid --owner-start-time: %w", err)
	}
	return parsed, nil
}

func parseProcStat(stat string) (byte, uint64, error) {
	closeName := strings.LastIndexByte(stat, ')')
	if closeName < 0 || closeName+1 >= len(stat) {
		return 0, 0, errors.New("malformed process stat")
	}

	// Fields after the executable name begin with field 3 (state). Linux
	// documents starttime as field 22, so it is index 19 in this slice.
	fields := strings.Fields(stat[closeName+1:])
	if len(fields) <= 19 || len(fields[0]) != 1 {
		return 0, 0, errors.New("process stat is missing state or start time")
	}
	startTime, err := strconv.ParseUint(fields[19], 10, 64)
	if err != nil || startTime == 0 {
		if err == nil {
			err = errors.New("start time is zero")
		}
		return 0, 0, fmt.Errorf("invalid process start time: %w", err)
	}
	return fields[0][0], startTime, nil
}

func parseProcStatusUIDs(status string) (uint32, uint32, error) {
	scanner := bufio.NewScanner(strings.NewReader(status))
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if !strings.HasPrefix(line, "Uid:") {
			continue
		}
		fields := strings.Fields(strings.TrimPrefix(line, "Uid:"))
		if len(fields) < 2 {
			return 0, 0, errors.New("process status has an incomplete Uid field")
		}
		realUID, err := strconv.ParseUint(fields[0], 10, 32)
		if err != nil {
			return 0, 0, fmt.Errorf("invalid real UID: %w", err)
		}
		effectiveUID, err := strconv.ParseUint(fields[1], 10, 32)
		if err != nil {
			return 0, 0, fmt.Errorf("invalid effective UID: %w", err)
		}
		return uint32(realUID), uint32(effectiveUID), nil
	}
	if err := scanner.Err(); err != nil {
		return 0, 0, err
	}
	return 0, 0, errors.New("process status is missing Uid")
}

func readOwnerSnapshot(procRoot string, pid int32) (ownerSnapshot, error) {
	processDirectory := fmt.Sprintf("%s/%d", strings.TrimSuffix(procRoot, "/"), pid)
	stat, err := os.ReadFile(processDirectory + "/stat")
	if err != nil {
		return ownerSnapshot{}, fmt.Errorf("read owner stat: %w", err)
	}
	status, err := os.ReadFile(processDirectory + "/status")
	if err != nil {
		return ownerSnapshot{}, fmt.Errorf("read owner status: %w", err)
	}

	state, startTime, err := parseProcStat(string(stat))
	if err != nil {
		return ownerSnapshot{}, err
	}
	realUID, effectiveUID, err := parseProcStatusUIDs(string(status))
	if err != nil {
		return ownerSnapshot{}, err
	}
	return ownerSnapshot{
		realUID:      realUID,
		effectiveUID: effectiveUID,
		startTime:    startTime,
		state:        state,
	}, nil
}

func validateOwner(procRoot string, owner ownerIdentity) error {
	snapshot, err := readOwnerSnapshot(procRoot, owner.pid)
	if err != nil {
		return err
	}
	if snapshot.realUID != owner.uid || snapshot.effectiveUID != owner.uid {
		return fmt.Errorf(
			"owner UID changed (real=%d effective=%d expected=%d)",
			snapshot.realUID,
			snapshot.effectiveUID,
			owner.uid,
		)
	}
	if snapshot.startTime != owner.startTime {
		return fmt.Errorf(
			"owner PID was reused (start=%d expected=%d)",
			snapshot.startTime,
			owner.startTime,
		)
	}
	if snapshot.state == 'Z' || snapshot.state == 'X' || snapshot.state == 'x' {
		return fmt.Errorf("owner process is no longer running (state=%c)", snapshot.state)
	}
	return nil
}

func monitorOwner(
	procRoot string,
	owner ownerIdentity,
	period time.Duration,
	stop <-chan struct{},
) error {
	ticker := time.NewTicker(period)
	defer ticker.Stop()

	for {
		select {
		case <-ticker.C:
			if err := validateOwner(procRoot, owner); err != nil {
				return err
			}
		case <-stop:
			return nil
		}
	}
}

func (h *nethelper) setupSocket() (*net.UnixListener, error) {
	if existing, err := net.DialTimeout("unix", h.socketPath, 200*time.Millisecond); err == nil {
		_ = existing.Close()
		return nil, fmt.Errorf("nethelper is already running for uid %d", h.owner.uid)
	}

	if err := os.Remove(h.socketPath); err != nil && !os.IsNotExist(err) {
		return nil, fmt.Errorf("remove stale socket: %w", err)
	}

	oldUmask := syscall.Umask(0077)
	addr := &net.UnixAddr{Name: h.socketPath, Net: "unix"}
	listener, err := net.ListenUnix("unix", addr)
	syscall.Umask(oldUmask)
	if err != nil {
		return nil, fmt.Errorf("listen: %w", err)
	}

	cleanup := func() {
		_ = listener.Close()
		_ = os.Remove(h.socketPath)
	}
	if err := os.Chown(h.socketPath, int(h.owner.uid), -1); err != nil {
		cleanup()
		return nil, fmt.Errorf("set socket owner: %w", err)
	}
	if err := os.Chmod(h.socketPath, 0600); err != nil {
		cleanup()
		return nil, fmt.Errorf("set socket permissions: %w", err)
	}
	return listener, nil
}

func peerCredentials(conn *net.UnixConn) (*syscall.Ucred, error) {
	raw, err := conn.SyscallConn()
	if err != nil {
		return nil, err
	}

	var credentials *syscall.Ucred
	var socketErr error
	if err := raw.Control(func(fd uintptr) {
		credentials, socketErr = syscall.GetsockoptUcred(
			int(fd),
			syscall.SOL_SOCKET,
			syscall.SO_PEERCRED,
		)
	}); err != nil {
		return nil, err
	}
	if socketErr != nil {
		return nil, socketErr
	}
	if credentials == nil {
		return nil, errors.New("missing peer credentials")
	}
	return credentials, nil
}

func parseBool(name, value string) (bool, error) {
	switch value {
	case "0":
		return false, nil
	case "1":
		return true, nil
	default:
		return false, fmt.Errorf("%s must be 0 or 1", name)
	}
}

func parseMode(value string) (targetMode, error) {
	switch value {
	case "all":
		return targetAll, nil
	case "roblox":
		return targetRoblox, nil
	case "custom":
		return targetCustom, nil
	default:
		return targetAll, fmt.Errorf("unsupported target mode %q", value)
	}
}

func parseIPs(value string) ([]string, error) {
	if value == "-" {
		return nil, nil
	}

	parts := strings.Split(value, ",")
	if len(parts) > maxTargets {
		return nil, fmt.Errorf("too many remote IPs (maximum %d)", maxTargets)
	}

	seen := make(map[string]struct{}, len(parts))
	ips := make([]string, 0, len(parts))
	for _, part := range parts {
		ip := net.ParseIP(part)
		if ip == nil || ip.To4() == nil {
			return nil, fmt.Errorf("invalid IPv4 address %q", part)
		}
		canonical := ip.To4().String()
		if _, exists := seen[canonical]; exists {
			continue
		}
		seen[canonical] = struct{}{}
		ips = append(ips, canonical)
	}
	return ips, nil
}

func parsePorts(value string) ([]int, error) {
	if value == "-" {
		return nil, nil
	}

	parts := strings.Split(value, ",")
	if len(parts) > maxTargets {
		return nil, fmt.Errorf("too many remote ports (maximum %d)", maxTargets)
	}

	seen := make(map[int]struct{}, len(parts))
	ports := make([]int, 0, len(parts))
	for _, part := range parts {
		port, err := strconv.Atoi(part)
		if err != nil || port < 1 || port > 65535 {
			return nil, fmt.Errorf("invalid remote port %q", part)
		}
		if _, exists := seen[port]; exists {
			continue
		}
		seen[port] = struct{}{}
		ports = append(ports, port)
	}
	return ports, nil
}

func parseBlock(parts []string) (blockConfig, error) {
	required := map[string]bool{
		"in": false, "out": false, "udp": false, "tcp": false,
		"mode": false, "ips": false, "ports": false,
	}
	values := make(map[string]string, len(parts))
	for _, part := range parts {
		pair := strings.SplitN(part, "=", 2)
		if len(pair) != 2 {
			return blockConfig{}, fmt.Errorf("invalid argument %q", part)
		}
		if _, known := required[pair[0]]; !known {
			return blockConfig{}, fmt.Errorf("unknown argument %q", pair[0])
		}
		if required[pair[0]] {
			return blockConfig{}, fmt.Errorf("duplicate argument %q", pair[0])
		}
		required[pair[0]] = true
		values[pair[0]] = pair[1]
	}
	for name, present := range required {
		if !present {
			return blockConfig{}, fmt.Errorf("missing argument %q", name)
		}
	}

	var cfg blockConfig
	var err error
	if cfg.inbound, err = parseBool("in", values["in"]); err != nil {
		return blockConfig{}, err
	}
	if cfg.outbound, err = parseBool("out", values["out"]); err != nil {
		return blockConfig{}, err
	}
	if !cfg.inbound && !cfg.outbound {
		return blockConfig{}, errors.New("at least one direction is required")
	}
	if cfg.udp, err = parseBool("udp", values["udp"]); err != nil {
		return blockConfig{}, err
	}
	if cfg.tcp, err = parseBool("tcp", values["tcp"]); err != nil {
		return blockConfig{}, err
	}
	if !cfg.udp && !cfg.tcp {
		return blockConfig{}, errors.New("at least one protocol is required")
	}
	if cfg.mode, err = parseMode(values["mode"]); err != nil {
		return blockConfig{}, err
	}
	if cfg.remoteIPs, err = parseIPs(values["ips"]); err != nil {
		return blockConfig{}, err
	}
	if cfg.remotePorts, err = parsePorts(values["ports"]); err != nil {
		return blockConfig{}, err
	}
	switch cfg.mode {
	case targetAll:
		if len(cfg.remoteIPs) != 0 || len(cfg.remotePorts) != 0 {
			return blockConfig{}, errors.New("targets are not valid in all-traffic mode")
		}
	case targetRoblox:
		if len(cfg.remotePorts) != 0 {
			return blockConfig{}, errors.New("ports are only valid in custom mode")
		}
	case targetCustom:
		if len(cfg.remoteIPs) == 0 && len(cfg.remotePorts) == 0 {
			return blockConfig{}, errors.New("custom mode requires an IP or port")
		}
	}
	return cfg, nil
}

func parseLag(parts []string) (lagConfig, error) {
	if len(parts) == 0 {
		return lagConfig{}, errors.New("lag requires a delay and traffic target")
	}
	blockParts := make([]string, 0, len(parts)-1)
	delayValue := ""
	for _, part := range parts {
		pair := strings.SplitN(part, "=", 2)
		if len(pair) != 2 {
			return lagConfig{}, fmt.Errorf("invalid argument %q", part)
		}
		if pair[0] == "delay" {
			if delayValue != "" {
				return lagConfig{}, errors.New("duplicate argument \"delay\"")
			}
			delayValue = pair[1]
			continue
		}
		blockParts = append(blockParts, part)
	}
	if delayValue == "" {
		return lagConfig{}, errors.New("missing argument \"delay\"")
	}
	delayMs, err := strconv.Atoi(delayValue)
	if err != nil || delayMs < 1 || delayMs > 5000 {
		return lagConfig{}, errors.New("delay must be between 1 and 5000 milliseconds")
	}
	block, err := parseBlock(blockParts)
	if err != nil {
		return lagConfig{}, err
	}
	return lagConfig{blockConfig: block, delayMs: delayMs}, nil
}

func rulesForDirection(cfg blockConfig, inbound bool) []ruleSpec {
	protocols := make([]string, 0, 2)
	if cfg.udp {
		protocols = append(protocols, "udp")
	}
	if cfg.tcp {
		protocols = append(protocols, "tcp")
	}

	addressFlag := "-d"
	portFlag := "--dport"
	if inbound {
		addressFlag = "-s"
		portFlag = "--sport"
	}

	var rules []ruleSpec
	for _, protocol := range protocols {
		switch cfg.mode {
		case targetAll:
			rules = append(rules, ruleSpec{"-p", protocol, "-j", "DROP"})
		case targetRoblox:
			rules = append(rules, ruleSpec{
				"-p", protocol, addressFlag, robloxIPv4CIDR, "-j", "DROP",
			})
			for _, ip := range cfg.remoteIPs {
				rules = append(rules, ruleSpec{
					"-p", protocol, addressFlag, ip, "-j", "DROP",
				})
			}
		case targetCustom:
			for _, ip := range cfg.remoteIPs {
				rules = append(rules, ruleSpec{
					"-p", protocol, addressFlag, ip, "-j", "DROP",
				})
			}
			for _, port := range cfg.remotePorts {
				rules = append(rules, ruleSpec{
					"-p", protocol, portFlag, strconv.Itoa(port), "-j", "DROP",
				})
			}
		}
	}
	return rules
}

func (h *nethelper) removeOwnedChain(builtin, chain string) error {
	var cleanupErrors []error
	chainExists, err := h.firewall.ChainExists("filter", chain)
	if err != nil {
		return fmt.Errorf("inspect %s: %w", chain, err)
	}
	if !chainExists {
		return nil
	}

	for {
		exists, err := h.firewall.Exists("filter", builtin, "-j", chain)
		if err != nil {
			cleanupErrors = append(cleanupErrors, fmt.Errorf("inspect %s jump: %w", builtin, err))
			break
		}
		if !exists {
			break
		}
		if err := h.firewall.Delete("filter", builtin, "-j", chain); err != nil {
			cleanupErrors = append(cleanupErrors, fmt.Errorf("delete %s jump: %w", builtin, err))
			break
		}
	}

	if err := h.firewall.ClearAndDeleteChain("filter", chain); err != nil {
		cleanupErrors = append(cleanupErrors, fmt.Errorf("delete %s: %w", chain, err))
	}
	return joinErrors(cleanupErrors...)
}

func (h *nethelper) resetOwned() error {
	return joinErrors(
		h.removeOwnedChain("INPUT", h.inputChain),
		h.removeOwnedChain("OUTPUT", h.outputChain),
		h.traffic.reset(),
	)
}

func (h *nethelper) installDirection(builtin, chain string, rules []ruleSpec) error {
	if err := h.firewall.NewChain("filter", chain); err != nil {
		return fmt.Errorf("create %s: %w", chain, err)
	}
	for _, rule := range rules {
		if err := h.firewall.Append("filter", chain, rule...); err != nil {
			return fmt.Errorf("add rule to %s: %w", chain, err)
		}
	}
	if err := h.firewall.InsertUnique("filter", builtin, 1, "-j", chain); err != nil {
		return fmt.Errorf("attach %s to %s: %w", chain, builtin, err)
	}
	return nil
}

func (h *nethelper) applyBlock(cfg blockConfig) error {
	if err := h.resetOwned(); err != nil {
		return err
	}

	var err error
	if cfg.inbound {
		err = h.installDirection("INPUT", h.inputChain, rulesForDirection(cfg, true))
	}
	if err == nil && cfg.outbound {
		err = h.installDirection("OUTPUT", h.outputChain, rulesForDirection(cfg, false))
	}
	if err != nil {
		return joinErrors(err, h.resetOwned())
	}
	return nil
}

func readCommand(conn *net.UnixConn) (string, error) {
	if err := conn.SetReadDeadline(time.Now().Add(2 * time.Second)); err != nil {
		return "", err
	}
	reader := bufio.NewReader(io.LimitReader(conn, maxCommandBytes+1))
	line, err := reader.ReadString('\n')
	if err != nil && !errors.Is(err, io.EOF) {
		return "", err
	}
	if len(line) > maxCommandBytes {
		return "", errors.New("command is too large")
	}
	line = strings.TrimSpace(line)
	if line == "" {
		return "", errors.New("empty command")
	}
	return line, nil
}

func writeResponse(conn *net.UnixConn, response string) {
	_ = conn.SetWriteDeadline(time.Now().Add(2 * time.Second))
	_, _ = io.WriteString(conn, response+"\n")
}

func safeError(err error) string {
	message := strings.ReplaceAll(err.Error(), "\n", " ")
	message = strings.ReplaceAll(message, "\r", " ")
	return message
}

func (h *nethelper) handle(conn *net.UnixConn) bool {
	defer conn.Close()

	credentials, err := peerCredentials(conn)
	if err != nil ||
		credentials.Uid != h.owner.uid ||
		credentials.Pid != h.owner.pid {
		writeResponse(conn, "ERR unauthorized")
		return true
	}

	command, err := readCommand(conn)
	if err != nil {
		writeResponse(conn, "ERR "+safeError(err))
		return true
	}

	parts := strings.Fields(command)
	switch parts[0] {
	case "ping":
		if len(parts) != 1 {
			writeResponse(conn, "ERR ping takes no arguments")
			return true
		}
		writeResponse(conn, "PONG")
	case "reset":
		if len(parts) != 1 {
			writeResponse(conn, "ERR reset takes no arguments")
			return true
		}
		if err := h.resetOwned(); err != nil {
			writeResponse(conn, "ERR "+safeError(err))
			return true
		}
		writeResponse(conn, "OK")
	case "shutdown":
		if len(parts) != 1 {
			writeResponse(conn, "ERR shutdown takes no arguments")
			return true
		}
		if err := h.resetOwned(); err != nil {
			writeResponse(conn, "ERR "+safeError(err))
			return true
		}
		writeResponse(conn, "OK")
		return false
	case "block":
		cfg, err := parseBlock(parts[1:])
		if err != nil {
			_ = h.resetOwned()
			writeResponse(conn, "ERR "+safeError(err))
			return true
		}
		if err := h.applyBlock(cfg); err != nil {
			writeResponse(conn, "ERR "+safeError(err))
			return true
		}
		writeResponse(conn, "OK")
	case "lag":
		cfg, err := parseLag(parts[1:])
		if err != nil {
			_ = h.resetOwned()
			writeResponse(conn, "ERR "+safeError(err))
			return true
		}
		if err := h.applyLag(cfg); err != nil {
			writeResponse(conn, "ERR "+safeError(err))
			return true
		}
		writeResponse(conn, "OK")
	default:
		writeResponse(conn, "ERR unknown command")
	}
	return true
}

func (h *nethelper) applyLag(cfg lagConfig) error {
	if err := h.removeOwnedChain("INPUT", h.inputChain); err != nil {
		return err
	}
	if err := h.removeOwnedChain("OUTPUT", h.outputChain); err != nil {
		return err
	}
	return h.traffic.applyLag(cfg)
}

func run() error {
	uidValue := flag.String("uid", "", "UID allowed to connect to this helper")
	ownerPIDValue := flag.String("owner-pid", "", "PID of the SMU GUI that owns this helper")
	ownerStartTimeValue := flag.String(
		"owner-start-time",
		"",
		"/proc stat start time of the SMU GUI that owns this helper",
	)
	flag.Parse()
	if *uidValue == "" {
		return errors.New("--uid is required")
	}
	if *ownerPIDValue == "" {
		return errors.New("--owner-pid is required")
	}
	if *ownerStartTimeValue == "" {
		return errors.New("--owner-start-time is required")
	}
	if flag.NArg() != 0 {
		return errors.New("unexpected positional arguments")
	}
	if os.Geteuid() != 0 {
		return errors.New("nethelper must run as root")
	}

	uid, err := parseUID(*uidValue)
	if err != nil {
		return err
	}
	ownerPID, err := parseOwnerPID(*ownerPIDValue)
	if err != nil {
		return err
	}
	ownerStartTime, err := parseOwnerStartTime(*ownerStartTimeValue)
	if err != nil {
		return err
	}
	owner := ownerIdentity{
		pid:       ownerPID,
		uid:       uid,
		startTime: ownerStartTime,
	}
	ipt, err := iptables.New()
	if err != nil {
		return fmt.Errorf("iptables: %w", err)
	}
	helper := newNethelper(owner, ipt)
	if err := helper.traffic.cleanupStale(); err != nil {
		return fmt.Errorf("clean stale SMU traffic-control state: %w", err)
	}
	if err := helper.resetOwned(); err != nil {
		return fmt.Errorf("clean stale SMU firewall state: %w", err)
	}
	if err := validateOwner("/proc", owner); err != nil {
		return fmt.Errorf("validate GUI owner: %w", err)
	}

	listener, err := helper.setupSocket()
	if err != nil {
		return err
	}
	defer listener.Close()
	defer os.Remove(helper.socketPath)
	defer func() {
		if err := helper.resetOwned(); err != nil {
			fmt.Fprintf(os.Stderr, "[nethelper] cleanup failed: %v\n", err)
		}
	}()

	stopBackground := make(chan struct{})
	defer close(stopBackground)
	ownerFailure := make(chan error, 1)
	go func() {
		if err := monitorOwner("/proc", owner, ownerPollPeriod, stopBackground); err != nil {
			ownerFailure <- err
			_ = listener.Close()
		}
	}()

	signals := make(chan os.Signal, 1)
	signal.Notify(signals, syscall.SIGINT, syscall.SIGTERM)
	defer signal.Stop(signals)
	go func() {
		select {
		case <-signals:
			_ = listener.Close()
		case <-stopBackground:
		}
	}()

	fmt.Printf(
		"[nethelper] uid=%d owner-pid=%d owner-start=%d socket=%s\n",
		uid,
		owner.pid,
		owner.startTime,
		helper.socketPath,
	)
	for {
		conn, err := listener.AcceptUnix()
		if err != nil {
			if errors.Is(err, net.ErrClosed) {
				select {
				case ownerErr := <-ownerFailure:
					return fmt.Errorf("GUI owner exited: %w", ownerErr)
				default:
				}
				return nil
			}
			fmt.Fprintf(os.Stderr, "[nethelper] accept failed: %v\n", err)
			continue
		}
		if !helper.handle(conn) {
			return nil
		}
	}
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "nethelper:", err)
		os.Exit(1)
	}
}
