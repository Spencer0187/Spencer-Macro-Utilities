package main

import (
	"bufio"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"
)

type ownerIdentity struct {
	pid       int
	uid       uint32
	startTime uint64
}
type helper struct {
	owner         ownerIdentity
	socketPath    string
	frozenParents map[int]string
}

func socketPath(uid uint32) string { return fmt.Sprintf("/tmp/smu-processhelper-%d.sock", uid) }

func procStartTime(pid int) (uint64, error) {
	data, err := os.ReadFile(fmt.Sprintf("/proc/%d/stat", pid))
	if err != nil {
		return 0, err
	}
	text := string(data)
	close := strings.LastIndex(text, ")")
	if close < 0 {
		return 0, errors.New("invalid proc stat")
	}
	fields := strings.Fields(text[close+1:])
	if len(fields) < 20 {
		return 0, errors.New("short proc stat")
	}
	return strconv.ParseUint(fields[19], 10, 64)
}

func processUID(pid int) (uint32, error) {
	data, err := os.ReadFile(fmt.Sprintf("/proc/%d/status", pid))
	if err != nil {
		return 0, err
	}
	for _, line := range strings.Split(string(data), "\n") {
		if strings.HasPrefix(line, "Uid:") {
			fields := strings.Fields(strings.TrimPrefix(line, "Uid:"))
			if len(fields) == 0 {
				break
			}
			value, err := strconv.ParseUint(fields[0], 10, 32)
			return uint32(value), err
		}
	}
	return 0, errors.New("process status did not contain Uid")
}

func validateOwner(owner ownerIdentity) error {
	uid, err := processUID(owner.pid)
	if err != nil {
		return err
	}
	if uid != owner.uid {
		return errors.New("owner uid changed")
	}
	start, err := procStartTime(owner.pid)
	if err != nil {
		return err
	}
	if start != owner.startTime {
		return errors.New("owner process identity changed")
	}
	return nil
}

func peerCredentials(conn *net.UnixConn) (*syscall.Ucred, error) {
	raw, err := conn.SyscallConn()
	if err != nil {
		return nil, err
	}
	var credentials *syscall.Ucred
	var controlErr error
	err = raw.Control(func(fd uintptr) {
		credentials, controlErr = syscall.GetsockoptUcred(int(fd), syscall.SOL_SOCKET, syscall.SO_PEERCRED)
	})
	if err != nil {
		return nil, err
	}
	return credentials, controlErr
}

func cgroupPath(pid int) (string, error) {
	data, err := os.ReadFile(fmt.Sprintf("/proc/%d/cgroup", pid))
	if err != nil {
		return "", err
	}
	for _, line := range strings.Split(string(data), "\n") {
		if strings.HasPrefix(line, "0::") {
			rel := filepath.Clean(strings.TrimPrefix(line, "0::"))
			if !strings.HasPrefix(rel, "/") || strings.Contains(rel, "..") {
				return "", errors.New("unsafe cgroup path")
			}
			return rel, nil
		}
	}
	return "", errors.New("target is not in cgroup v2")
}

func fullCgroupPath(relative string) (string, error) {
	clean := filepath.Clean(relative)
	if !strings.HasPrefix(clean, "/") || strings.Contains(clean, "..") {
		return "", errors.New("unsafe cgroup path")
	}
	root := "/sys/fs/cgroup"
	full := filepath.Join(root, strings.TrimPrefix(clean, "/"))
	if full != root && !strings.HasPrefix(full, root+string(os.PathSeparator)) {
		return "", errors.New("cgroup path escaped root")
	}
	return full, nil
}

func writeControl(path, value string) error {
	file, err := os.OpenFile(path, os.O_WRONLY, 0)
	if err != nil {
		return err
	}
	defer file.Close()
	_, err = io.WriteString(file, value)
	return err
}

func (h *helper) validateTarget(pid int) error {
	if pid <= 0 {
		return errors.New("invalid PID")
	}
	uid, err := processUID(pid)
	if err != nil {
		return err
	}
	if uid != h.owner.uid {
		return errors.New("target is owned by another uid")
	}
	comm, err := os.ReadFile(fmt.Sprintf("/proc/%d/comm", pid))
	if err != nil {
		return err
	}
	if !strings.EqualFold(strings.TrimSpace(string(comm)), "Main") {
		return errors.New("target is not Sober Main")
	}
	exe, err := os.Readlink(fmt.Sprintf("/proc/%d/exe", pid))
	if err != nil {
		return err
	}
	if !strings.EqualFold(filepath.Base(exe), "sober") {
		return errors.New("target executable is not sober")
	}
	return nil
}

func (h *helper) freeze(pid int) error {
	if err := h.validateTarget(pid); err != nil {
		return err
	}
	relative, err := cgroupPath(pid)
	if err != nil {
		return err
	}
	current, err := fullCgroupPath(relative)
	if err != nil {
		return err
	}
	if filepath.Base(current) == "smu_freeze" {
		h.frozenParents[pid] = filepath.Dir(current)
		return writeControl(filepath.Join(current, "cgroup.freeze"), "1")
	}
	child := filepath.Join(current, "smu_freeze")
	if err := os.Mkdir(child, 0755); err != nil && !errors.Is(err, os.ErrExist) {
		return fmt.Errorf("create freeze cgroup: %w", err)
	}
	if err := writeControl(filepath.Join(child, "cgroup.procs"), strconv.Itoa(pid)); err != nil {
		_ = os.Remove(child)
		return fmt.Errorf("move target: %w", err)
	}
	if err := writeControl(filepath.Join(child, "cgroup.freeze"), "1"); err != nil {
		_ = writeControl(filepath.Join(current, "cgroup.procs"), strconv.Itoa(pid))
		_ = os.Remove(child)
		return fmt.Errorf("freeze target: %w", err)
	}
	h.frozenParents[pid] = current
	return nil
}

func (h *helper) thaw(pid int) error {
	if err := h.validateTarget(pid); err != nil {
		return err
	}
	relative, err := cgroupPath(pid)
	if err != nil {
		return err
	}
	current, err := fullCgroupPath(relative)
	if err != nil {
		return err
	}
	if filepath.Base(current) != "smu_freeze" {
		delete(h.frozenParents, pid)
		return errors.New("target is not in SMU freeze cgroup")
	}
	parent := h.frozenParents[pid]
	if parent == "" {
		parent = filepath.Dir(current)
	}
	if err := writeControl(filepath.Join(current, "cgroup.freeze"), "0"); err != nil {
		return err
	}
	if err := writeControl(filepath.Join(parent, "cgroup.procs"), strconv.Itoa(pid)); err != nil {
		return err
	}
	delete(h.frozenParents, pid)
	_ = os.Remove(current)
	return nil
}

func (h *helper) cleanup() {
	for pid := range h.frozenParents {
		_ = h.thaw(pid)
	}
}

func (h *helper) handle(conn *net.UnixConn) bool {
	defer conn.Close()
	credentials, err := peerCredentials(conn)
	if err != nil || credentials.Uid != h.owner.uid || int(credentials.Pid) != h.owner.pid {
		_, _ = io.WriteString(conn, "ERR unauthorized\n")
		return true
	}
	_ = conn.SetReadDeadline(time.Now().Add(2 * time.Second))
	line, err := bufio.NewReader(io.LimitReader(conn, 1024)).ReadString('\n')
	if err != nil {
		_, _ = io.WriteString(conn, "ERR invalid command\n")
		return true
	}
	fields := strings.Fields(line)
	if len(fields) == 0 {
		_, _ = io.WriteString(conn, "ERR empty command\n")
		return true
	}
	var result error
	switch fields[0] {
	case "ping":
		if len(fields) != 1 {
			result = errors.New("ping takes no arguments")
		} else {
			_, _ = io.WriteString(conn, "PONG\n")
			return true
		}
	case "freeze":
		if len(fields) != 3 {
			result = errors.New("freeze requires PID and state")
			break
		}
		pid, err := strconv.Atoi(fields[1])
		if err != nil {
			result = errors.New("invalid PID")
			break
		}
		if fields[2] == "1" {
			result = h.freeze(pid)
		} else if fields[2] == "0" {
			result = h.thaw(pid)
		} else {
			result = errors.New("state must be 0 or 1")
		}
	default:
		result = errors.New("unknown command")
	}
	if result != nil {
		_, _ = io.WriteString(conn, "ERR "+strings.ReplaceAll(result.Error(), "\n", " ")+"\n")
	} else {
		_, _ = io.WriteString(conn, "OK\n")
	}
	return true
}

func main() {
	uidFlag := flag.String("uid", "", "owner uid")
	pidFlag := flag.Int("owner-pid", 0, "owner pid")
	startFlag := flag.Uint64("owner-start-time", 0, "owner start time")
	flag.Parse()
	if os.Geteuid() != 0 || *uidFlag == "" || *pidFlag <= 0 || *startFlag == 0 {
		fmt.Fprintln(os.Stderr, "processhelper: invalid privileged invocation")
		os.Exit(1)
	}
	uid64, err := strconv.ParseUint(*uidFlag, 10, 32)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	h := &helper{owner: ownerIdentity{pid: *pidFlag, uid: uint32(uid64), startTime: *startFlag}, socketPath: socketPath(uint32(uid64)), frozenParents: map[int]string{}}
	if err := validateOwner(h.owner); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	_ = os.Remove(h.socketPath)
	oldUmask := syscall.Umask(0077)
	address := &net.UnixAddr{Name: h.socketPath, Net: "unix"}
	listener, err := net.ListenUnix("unix", address)
	syscall.Umask(oldUmask)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	defer listener.Close()
	defer os.Remove(h.socketPath)
	defer h.cleanup()
	if err := os.Chown(h.socketPath, int(h.owner.uid), -1); err != nil {
		fmt.Fprintln(os.Stderr, "set socket owner:", err)
		os.Exit(1)
	}
	if err := os.Chmod(h.socketPath, 0600); err != nil {
		fmt.Fprintln(os.Stderr, "set socket permissions:", err)
		os.Exit(1)
	}
	for {
		if err := validateOwner(h.owner); err != nil {
			return
		}
		_ = listener.SetDeadline(time.Now().Add(time.Second))
		conn, err := listener.AcceptUnix()
		if ne, ok := err.(net.Error); ok && ne.Timeout() {
			continue
		}
		if err != nil {
			return
		}
		if !h.handle(conn) {
			return
		}
	}
}
