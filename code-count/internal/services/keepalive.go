package services

import (
	"sync"
	"time"

	"github.com/getlantern/systray"
)

// KeepAliveManager 保活管理器
// @author l10840, date 2025-09-24
type KeepAliveManager struct {
	lastHeartbeat time.Time
	mutex         sync.RWMutex
	running       bool
	stopChan      chan struct{}
}

// NewKeepAliveManager 创建保活管理器
// @author l10840, date 2025-09-24
func NewKeepAliveManager() *KeepAliveManager {
	return &KeepAliveManager{
		lastHeartbeat: time.Now(),
		stopChan:      make(chan struct{}),
	}
}

// UpdateHeartbeat 更新心跳时间
// @author l10840, date 2025-09-24
func (k *KeepAliveManager) UpdateHeartbeat() {
	k.mutex.Lock()
	defer k.mutex.Unlock()
	k.lastHeartbeat = time.Now()
}

// Start 启动保活检测
// @author l10840, date 2025-09-24
func (k *KeepAliveManager) Start() {
	k.mutex.Lock()
	if k.running {
		k.mutex.Unlock()
		return
	}
	k.running = true
	k.mutex.Unlock()

	go k.monitorHeartbeat()
}

// Stop 停止保活检测
// @author l10840, date 2025-09-24
func (k *KeepAliveManager) Stop() {
	k.mutex.Lock()
	defer k.mutex.Unlock()
	if !k.running {
		return
	}
	k.running = false
	close(k.stopChan)
}

// monitorHeartbeat 监控心跳
// @author l10840, date 2025-09-24
func (k *KeepAliveManager) monitorHeartbeat() {
	ticker := time.NewTicker(30 * time.Second) // 每30秒检查一次
	defer ticker.Stop()

	missedCount := 0
	const maxMissed = 5 // 最多允许错过5次心跳

	for {
		select {
		case <-ticker.C:
			k.mutex.RLock()
			lastTime := k.lastHeartbeat
			k.mutex.RUnlock()

			// 检查是否超过30秒没有心跳
			if time.Since(lastTime) > 30*time.Second {
				missedCount++
				if missedCount >= maxMissed {
					// 连续5次没有收到心跳，退出应用
					go func() {
						systray.Quit()
					}()
					return
				}
			} else {
				// 收到心跳，重置计数
				missedCount = 0
			}

		case <-k.stopChan:
			return
		}
	}
}