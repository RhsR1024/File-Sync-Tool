package services

import (
	"fmt"
	"sync"
	"time"

	"code-count/internal/models"

	"github.com/google/uuid"
)

// TaskManager 任务管理器
// @author l10840, date 2025-09-18
type TaskManager struct {
	tasks map[string]*models.Task
	mutex sync.RWMutex
}

// NewTaskManager 创建任务管理器
// @author l10840, date 2025-09-18
func NewTaskManager() *TaskManager {
	return &TaskManager{
		tasks: make(map[string]*models.Task),
	}
}

// CreateTask 创建新任务
// @author l10840, date 2025-09-18
func (tm *TaskManager) CreateTask(oldPath, newPath string) string {
	taskID := uuid.New().String()

	task := &models.Task{
		TaskID:    taskID,
		Status:    "running",
		StartTime: time.Now(),
		Progress: models.Progress{
			Phase:          "scan",
			CurrentFile:    "",
			ProcessedFiles: 0,
			TotalFiles:     0,
			Percent:        0,
		},
	}

	tm.mutex.Lock()
	tm.tasks[taskID] = task
	tm.mutex.Unlock()

	// 启动异步任务处理
	go tm.processTask(taskID, oldPath, newPath)

	return taskID
}

// GetTask 获取任务信息
// @author l10840, date 2025-09-18
func (tm *TaskManager) GetTask(taskID string) (*models.Task, error) {
	tm.mutex.RLock()
	defer tm.mutex.RUnlock()

	task, exists := tm.tasks[taskID]
	if !exists {
		return nil, fmt.Errorf("task not found")
	}

	// 返回任务的副本，避免并发问题
	taskCopy := *task
	return &taskCopy, nil
}

// GetProgress 获取任务进度
// @author l10840, date 2025-09-18
func (tm *TaskManager) GetProgress(taskID string) (*models.Progress, error) {
	tm.mutex.RLock()
	defer tm.mutex.RUnlock()

	task, exists := tm.tasks[taskID]
	if !exists {
		return nil, fmt.Errorf("task not found")
	}

	progressCopy := task.Progress
	return &progressCopy, nil
}

// GetResult 获取任务结果
// @author l10840, date 2025-09-18
func (tm *TaskManager) GetResult(taskID string) (*models.TaskResult, error) {
	tm.mutex.RLock()
	defer tm.mutex.RUnlock()

	task, exists := tm.tasks[taskID]
	if !exists {
		return nil, fmt.Errorf("task not found")
	}

	if task.Status != "completed" {
		return nil, fmt.Errorf("task not completed yet")
	}

	return &task.Result, nil
}

// processTask 处理任务的具体逻辑
// @author l10840, date 2025-09-18
func (tm *TaskManager) processTask(taskID, oldPath, newPath string) {
	// 创建进度回调函数
	progressCallback := func(phase, currentFile string, processed, total int) {
		tm.updateProgress(taskID, phase, currentFile, processed, total)
	}

	// 创建文件扫描器
	scanner := NewFileScanner(progressCallback)

	// 执行目录对比
	result, err := scanner.CompareDirectories(oldPath, newPath)

	tm.mutex.Lock()
	defer tm.mutex.Unlock()

	task := tm.tasks[taskID]
	if err != nil {
		task.Status = "failed"
		task.Error = err.Error()
	} else {
		task.Status = "completed"
		task.Result = *result
		task.Progress.Phase = "completed"
		task.Progress.Percent = 100
	}

	endTime := time.Now()
	task.EndTime = &endTime
}

// updateProgress 更新任务进度
// @author l10840, date 2025-09-18
func (tm *TaskManager) updateProgress(taskID, phase, currentFile string, processed, total int) {
	tm.mutex.Lock()
	defer tm.mutex.Unlock()

	task, exists := tm.tasks[taskID]
	if !exists {
		return
	}

	task.Progress.Phase = phase
	task.Progress.CurrentFile = currentFile
	task.Progress.ProcessedFiles = processed
	task.Progress.TotalFiles = total

	if total > 0 {
		task.Progress.Percent = (processed * 100) / total
	} else {
		task.Progress.Percent = 0
	}
}

// CleanupTask 清理已完成的任务（可选，用于内存管理）
// @author l10840, date 2025-09-18
func (tm *TaskManager) CleanupTask(taskID string) {
	tm.mutex.Lock()
	defer tm.mutex.Unlock()

	delete(tm.tasks, taskID)
}

// GetAllTasks 获取所有任务（用于调试或管理）
// @author l10840, date 2025-09-18
func (tm *TaskManager) GetAllTasks() map[string]*models.Task {
	tm.mutex.RLock()
	defer tm.mutex.RUnlock()

	tasks := make(map[string]*models.Task)
	for id, task := range tm.tasks {
		taskCopy := *task
		tasks[id] = &taskCopy
	}

	return tasks
}