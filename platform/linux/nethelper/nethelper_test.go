package main

import (
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"
)

func TestPerUserNames(t *testing.T) {
	const uid uint32 = 1001
	if got, want := socketPathForUID(uid), "/tmp/smu-nethelper-1001.sock"; got != want {
		t.Fatalf("socket path = %q, want %q", got, want)
	}
	input, output := chainNamesForUID(uid)
	if input != "SMU_IN_1001" || output != "SMU_OUT_1001" {
		t.Fatalf("chain names = %q, %q", input, output)
	}
}

func fakeProcStat(pid int32, state byte, startTime uint64) string {
	fields := []string{string(state)}
	for len(fields) < 19 {
		fields = append(fields, "0")
	}
	fields = append(fields, strconv.FormatUint(startTime, 10))
	return fmt.Sprintf(
		"%d (SMU owner ) with spaces) %s\n",
		pid,
		strings.Join(fields, " "),
	)
}

func writeFakeOwnerProcess(
	t *testing.T,
	procRoot string,
	owner ownerIdentity,
	state byte,
	startTime uint64,
	realUID uint32,
	effectiveUID uint32,
) {
	t.Helper()
	processDirectory := filepath.Join(procRoot, strconv.FormatInt(int64(owner.pid), 10))
	if err := os.MkdirAll(processDirectory, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(
		filepath.Join(processDirectory, "stat"),
		[]byte(fakeProcStat(owner.pid, state, startTime)),
		0600,
	); err != nil {
		t.Fatal(err)
	}
	status := fmt.Sprintf(
		"Name:\tsuspend\nState:\t%c\nUid:\t%d\t%d\t%d\t%d\n",
		state,
		realUID,
		effectiveUID,
		realUID,
		realUID,
	)
	if err := os.WriteFile(
		filepath.Join(processDirectory, "status"),
		[]byte(status),
		0600,
	); err != nil {
		t.Fatal(err)
	}
}

func TestOwnerIdentityValidationRejectsPidReuseUidChangeAndZombie(t *testing.T) {
	owner := ownerIdentity{pid: 4242, uid: 1001, startTime: 9001}
	procRoot := t.TempDir()

	writeFakeOwnerProcess(t, procRoot, owner, 'S', owner.startTime, owner.uid, owner.uid)
	if err := validateOwner(procRoot, owner); err != nil {
		t.Fatalf("valid owner rejected: %v", err)
	}

	writeFakeOwnerProcess(t, procRoot, owner, 'S', owner.startTime+1, owner.uid, owner.uid)
	if err := validateOwner(procRoot, owner); err == nil ||
		!strings.Contains(err.Error(), "PID was reused") {
		t.Fatalf("PID reuse error = %v", err)
	}

	writeFakeOwnerProcess(t, procRoot, owner, 'S', owner.startTime, owner.uid, 0)
	if err := validateOwner(procRoot, owner); err == nil ||
		!strings.Contains(err.Error(), "UID changed") {
		t.Fatalf("UID change error = %v", err)
	}

	writeFakeOwnerProcess(t, procRoot, owner, 'Z', owner.startTime, owner.uid, owner.uid)
	if err := validateOwner(procRoot, owner); err == nil ||
		!strings.Contains(err.Error(), "no longer running") {
		t.Fatalf("zombie error = %v", err)
	}
}

func TestOwnerMonitorDetectsStartTimeChange(t *testing.T) {
	owner := ownerIdentity{pid: 4343, uid: 1002, startTime: 81234}
	procRoot := t.TempDir()
	writeFakeOwnerProcess(t, procRoot, owner, 'S', owner.startTime, owner.uid, owner.uid)

	stop := make(chan struct{})
	result := make(chan error, 1)
	go func() {
		result <- monitorOwner(procRoot, owner, time.Millisecond, stop)
	}()

	writeFakeOwnerProcess(t, procRoot, owner, 'S', owner.startTime+1, owner.uid, owner.uid)
	select {
	case err := <-result:
		if err == nil || !strings.Contains(err.Error(), "PID was reused") {
			t.Fatalf("monitor error = %v", err)
		}
	case <-time.After(time.Second):
		close(stop)
		t.Fatal("owner monitor did not detect PID reuse")
	}
}

func TestCurrentProcessOwnerIdentity(t *testing.T) {
	stat, err := os.ReadFile("/proc/self/stat")
	if err != nil {
		t.Skipf("/proc/self/stat is unavailable: %v", err)
	}
	_, startTime, err := parseProcStat(string(stat))
	if err != nil {
		t.Fatalf("parseProcStat() error = %v", err)
	}
	owner := ownerIdentity{
		pid:       int32(os.Getpid()),
		uid:       uint32(os.Getuid()),
		startTime: startTime,
	}
	if err := validateOwner("/proc", owner); err != nil {
		t.Fatalf("current process owner rejected: %v", err)
	}
}

func TestParseBlockRequiresExplicitSafeTargets(t *testing.T) {
	cfg, err := parseBlock(strings.Fields(
		"in=1 out=0 udp=1 tcp=0 mode=custom ips=203.0.113.8 ports=443",
	))
	if err != nil {
		t.Fatalf("parseBlock() error = %v", err)
	}
	if !cfg.inbound || cfg.outbound || !cfg.udp || cfg.tcp ||
		cfg.mode != targetCustom ||
		!reflect.DeepEqual(cfg.remoteIPs, []string{"203.0.113.8"}) ||
		!reflect.DeepEqual(cfg.remotePorts, []int{443}) {
		t.Fatalf("unexpected config: %#v", cfg)
	}

	invalid := []string{
		"in=1 out=1 udp=1 tcp=0 mode=custom ips=- ports=-",
		"in=0 out=0 udp=1 tcp=0 mode=all ips=- ports=-",
		"in=1 out=1 udp=0 tcp=0 mode=all ips=- ports=-",
		"in=1 out=1 udp=1 tcp=0 mode=roblox ips=- ports=443",
		"in=1 out=1 udp=1 tcp=0 mode=custom ips=not-an-ip ports=-",
		"in=1 out=1 udp=1 tcp=0 mode=custom ips=- ports=70000",
	}
	for _, command := range invalid {
		if _, err := parseBlock(strings.Fields(command)); err == nil {
			t.Errorf("parseBlock(%q) unexpectedly succeeded", command)
		}
	}
}

func TestRulesHonorDirectionProtocolAndCustomTargets(t *testing.T) {
	cfg := blockConfig{
		udp:         true,
		tcp:         false,
		mode:        targetCustom,
		remoteIPs:   []string{"203.0.113.8"},
		remotePorts: []int{443},
	}
	inbound := rulesForDirection(cfg, true)
	wantInbound := []ruleSpec{
		{"-p", "udp", "-s", "203.0.113.8", "-j", "DROP"},
		{"-p", "udp", "--sport", "443", "-j", "DROP"},
	}
	if !reflect.DeepEqual(inbound, wantInbound) {
		t.Fatalf("inbound rules = %#v, want %#v", inbound, wantInbound)
	}

	cfg.udp = false
	cfg.tcp = true
	outbound := rulesForDirection(cfg, false)
	wantOutbound := []ruleSpec{
		{"-p", "tcp", "-d", "203.0.113.8", "-j", "DROP"},
		{"-p", "tcp", "--dport", "443", "-j", "DROP"},
	}
	if !reflect.DeepEqual(outbound, wantOutbound) {
		t.Fatalf("outbound rules = %#v, want %#v", outbound, wantOutbound)
	}
}

func unixSocketPair(t *testing.T) (*net.UnixConn, *net.UnixConn) {
	t.Helper()

	descriptors, err := syscall.Socketpair(syscall.AF_UNIX, syscall.SOCK_STREAM, 0)
	if err != nil {
		if errors.Is(err, syscall.EPERM) {
			t.Skip("Unix sockets are blocked by the test sandbox")
		}
		t.Fatal(err)
	}
	leftFile := os.NewFile(uintptr(descriptors[0]), "nethelper-test-left")
	rightFile := os.NewFile(uintptr(descriptors[1]), "nethelper-test-right")
	if leftFile == nil || rightFile == nil {
		t.Fatal("could not wrap Unix socket descriptors")
	}

	leftConnection, err := net.FileConn(leftFile)
	_ = leftFile.Close()
	if err != nil {
		_ = rightFile.Close()
		if errors.Is(err, syscall.EPERM) {
			t.Skip("Unix socket inspection is blocked by the test sandbox")
		}
		t.Fatal(err)
	}
	rightConnection, err := net.FileConn(rightFile)
	_ = rightFile.Close()
	if err != nil {
		_ = leftConnection.Close()
		if errors.Is(err, syscall.EPERM) {
			t.Skip("Unix socket inspection is blocked by the test sandbox")
		}
		t.Fatal(err)
	}

	left, leftOK := leftConnection.(*net.UnixConn)
	right, rightOK := rightConnection.(*net.UnixConn)
	if !leftOK || !rightOK {
		_ = leftConnection.Close()
		_ = rightConnection.Close()
		t.Fatal("socket pair did not produce Unix connections")
	}
	return left, right
}

func TestPeerCredentialsUseKernelIdentity(t *testing.T) {
	client, server := unixSocketPair(t)
	defer client.Close()
	defer server.Close()

	credentials, err := peerCredentials(server)
	if err != nil {
		t.Fatalf("peerCredentials() error = %v", err)
	}
	if credentials == nil {
		t.Fatal("peer credentials are nil")
	}
	if want := uint32(os.Getuid()); credentials.Uid != want {
		t.Fatalf("peer UID = %d, want %d", credentials.Uid, want)
	}
	if want := int32(os.Getpid()); credentials.Pid != want {
		t.Fatalf("peer PID = %d, want %d", credentials.Pid, want)
	}
}

func exchangeWithHelper(t *testing.T, helper *nethelper, command string) string {
	t.Helper()

	client, server := unixSocketPair(t)
	defer client.Close()

	handled := make(chan bool, 1)
	go func() {
		handled <- helper.handle(server)
	}()

	if err := client.SetDeadline(time.Now().Add(time.Second)); err != nil {
		t.Fatal(err)
	}
	if _, err := client.Write([]byte(command + "\n")); err != nil {
		t.Fatal(err)
	}

	buffer := make([]byte, 512)
	count, err := client.Read(buffer)
	if err != nil {
		t.Fatal(err)
	}
	if keepRunning := <-handled; !keepRunning {
		t.Fatal("helper unexpectedly requested shutdown")
	}
	return strings.TrimSpace(string(buffer[:count]))
}

func TestHandleRequiresExactOwnerProcess(t *testing.T) {
	owner := ownerIdentity{
		pid: int32(os.Getpid()),
		uid: uint32(os.Getuid()),
	}
	helper := newNethelper(owner, newFakeFirewall())
	if got := exchangeWithHelper(t, helper, "ping"); got != "PONG" {
		t.Fatalf("authorized ping response = %q", got)
	}

	helper.owner.pid++
	if got := exchangeWithHelper(t, helper, "ping"); got != "ERR unauthorized" {
		t.Fatalf("wrong-PID ping response = %q", got)
	}

	helper.owner = owner
	helper.owner.uid++
	if got := exchangeWithHelper(t, helper, "ping"); got != "ERR unauthorized" {
		t.Fatalf("wrong-UID ping response = %q", got)
	}
}

type fakeFirewall struct {
	chains map[string]bool
	rules  map[string][][]string
}

func newFakeFirewall() *fakeFirewall {
	return &fakeFirewall{
		chains: map[string]bool{
			"INPUT":  true,
			"OUTPUT": true,
		},
		rules: make(map[string][][]string),
	}
}

func copyRule(rule []string) []string {
	return append([]string(nil), rule...)
}

func (f *fakeFirewall) Append(_ string, chain string, rulespec ...string) error {
	if !f.chains[chain] {
		return fmt.Errorf("chain %s does not exist", chain)
	}
	f.rules[chain] = append(f.rules[chain], copyRule(rulespec))
	return nil
}

func (f *fakeFirewall) ChainExists(_, chain string) (bool, error) {
	return f.chains[chain], nil
}

func (f *fakeFirewall) ClearAndDeleteChain(_, chain string) error {
	if !f.chains[chain] {
		return nil
	}
	delete(f.rules, chain)
	delete(f.chains, chain)
	return nil
}

func (f *fakeFirewall) Delete(_ string, chain string, rulespec ...string) error {
	rules := f.rules[chain]
	for index, rule := range rules {
		if reflect.DeepEqual(rule, rulespec) {
			f.rules[chain] = append(rules[:index], rules[index+1:]...)
			return nil
		}
	}
	return fmt.Errorf("rule not found")
}

func (f *fakeFirewall) Exists(_ string, chain string, rulespec ...string) (bool, error) {
	for _, rule := range f.rules[chain] {
		if reflect.DeepEqual(rule, rulespec) {
			return true, nil
		}
	}
	return false, nil
}

func (f *fakeFirewall) InsertUnique(
	_ string,
	chain string,
	position int,
	rulespec ...string,
) error {
	exists, _ := f.Exists("filter", chain, rulespec...)
	if exists {
		return nil
	}
	if position != 1 {
		return fmt.Errorf("fake firewall only supports position 1")
	}
	f.rules[chain] = append([][]string{copyRule(rulespec)}, f.rules[chain]...)
	return nil
}

func (f *fakeFirewall) NewChain(_, chain string) error {
	if f.chains[chain] {
		return fmt.Errorf("chain %s already exists", chain)
	}
	f.chains[chain] = true
	return nil
}

func TestApplyAndResetPreserveUnrelatedFirewallRules(t *testing.T) {
	firewall := newFakeFirewall()
	firewall.rules["INPUT"] = [][]string{{"-p", "tcp", "-j", "ACCEPT"}}
	firewall.rules["OUTPUT"] = [][]string{{"-d", "192.0.2.1", "-j", "REJECT"}}
	originalInput := copyRule(firewall.rules["INPUT"][0])
	originalOutput := copyRule(firewall.rules["OUTPUT"][0])

	helper := newNethelper(ownerIdentity{uid: 1001}, firewall)
	cfg := blockConfig{
		inbound:  true,
		outbound: true,
		udp:      true,
		mode:     targetRoblox,
	}
	if err := helper.applyBlock(cfg); err != nil {
		t.Fatalf("applyBlock() error = %v", err)
	}

	if !firewall.chains[helper.inputChain] || !firewall.chains[helper.outputChain] {
		t.Fatal("SMU-owned chains were not created")
	}
	if !reflect.DeepEqual(firewall.rules["INPUT"][1], originalInput) {
		t.Fatalf("unrelated INPUT rule changed: %#v", firewall.rules["INPUT"])
	}
	if !reflect.DeepEqual(firewall.rules["OUTPUT"][1], originalOutput) {
		t.Fatalf("unrelated OUTPUT rule changed: %#v", firewall.rules["OUTPUT"])
	}

	if err := helper.resetOwned(); err != nil {
		t.Fatalf("resetOwned() error = %v", err)
	}
	if firewall.chains[helper.inputChain] || firewall.chains[helper.outputChain] {
		t.Fatal("SMU-owned chains survived reset")
	}
	if !reflect.DeepEqual(firewall.rules["INPUT"], [][]string{originalInput}) {
		t.Fatalf("reset changed unrelated INPUT rules: %#v", firewall.rules["INPUT"])
	}
	if !reflect.DeepEqual(firewall.rules["OUTPUT"], [][]string{originalOutput}) {
		t.Fatalf("reset changed unrelated OUTPUT rules: %#v", firewall.rules["OUTPUT"])
	}
}

func TestInvalidFakeLagCommandFailsClosedAndRemovesOwnedRules(t *testing.T) {
	owner := ownerIdentity{
		pid: int32(os.Getpid()),
		uid: uint32(os.Getuid()),
	}
	firewall := newFakeFirewall()
	helper := newNethelper(owner, firewall)
	if err := helper.applyBlock(blockConfig{
		inbound: true,
		udp:     true,
		mode:    targetAll,
	}); err != nil {
		t.Fatalf("applyBlock() error = %v", err)
	}

	response := exchangeWithHelper(t, helper, "lag delay=bad in=1 out=1 udp=1 tcp=0 mode=all ips=- ports=-")
	if !strings.Contains(response, "ERR delay must be between 1 and 5000 milliseconds") {
		t.Fatalf("fake-lag response = %q", response)
	}
	if firewall.chains[helper.inputChain] || firewall.chains[helper.outputChain] {
		t.Fatal("SMU-owned firewall chains survived a rejected fake-lag command")
	}
}

func TestParseLagAcceptsBoundedTargetedDelay(t *testing.T) {
	cfg, err := parseLag([]string{
		"delay=125", "in=1", "out=0", "udp=1", "tcp=0",
		"mode=roblox", "ips=-", "ports=-",
	})
	if err != nil {
		t.Fatalf("parseLag() error = %v", err)
	}
	if cfg.delayMs != 125 || !cfg.inbound || cfg.outbound || cfg.mode != targetRoblox {
		t.Fatalf("parseLag() = %#v", cfg)
	}
}

func TestParseRobloxAcceptsDiscoveredServerIPs(t *testing.T) {
	cfg, err := parseBlock([]string{
		"in=1", "out=1", "udp=1", "tcp=0",
		"mode=roblox", "ips=128.116.32.33,10.18.8.227", "ports=-",
	})
	if err != nil {
		t.Fatalf("parseBlock() error = %v", err)
	}
	if !reflect.DeepEqual(cfg.remoteIPs, []string{"128.116.32.33", "10.18.8.227"}) {
		t.Fatalf("remoteIPs = %#v", cfg.remoteIPs)
	}
}
