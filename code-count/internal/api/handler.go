package api

import (
	"fmt"
	"net/http"
	"os"
	"time"

	"code-count/internal/models"
	"code-count/internal/services"

	"github.com/gin-gonic/gin"
	"github.com/getlantern/systray"
)

// Handler API处理器
// @author l10840, date 2025-09-18
type Handler struct {
	taskManager     *services.TaskManager
	keepAliveManager *services.KeepAliveManager
}

// NewHandler 创建API处理器
// @author l10840, date 2025-09-18
func NewHandler(taskManager *services.TaskManager, keepAliveManager *services.KeepAliveManager) *Handler {
	return &Handler{
		taskManager:     taskManager,
		keepAliveManager: keepAliveManager,
	}
}

// CreateTask 创建任务接口
// @author l10840, date 2025-09-18
func (h *Handler) CreateTask(c *gin.Context) {
	var req models.TaskRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{
			"error":   "Invalid request format",
			"details": err.Error(),
		})
		return
	}

	// 验证路径是否存在
	if !isPathExists(req.OldPath) {
		c.JSON(http.StatusBadRequest, gin.H{
			"error": "Old path does not exist",
		})
		return
	}

	if !isPathExists(req.NewPath) {
		c.JSON(http.StatusBadRequest, gin.H{
			"error": "New path does not exist",
		})
		return
	}

	taskID := h.taskManager.CreateTask(req.OldPath, req.NewPath)

	c.JSON(http.StatusOK, models.TaskResponse{
		TaskID: taskID,
	})
}

// GetProgress 获取任务进度
// @author l10840, date 2025-09-18
func (h *Handler) GetProgress(c *gin.Context) {
	taskID := c.Param("taskId")
	if taskID == "" {
		c.JSON(http.StatusBadRequest, gin.H{
			"error": "Task ID is required",
		})
		return
	}

	task, err := h.taskManager.GetTask(taskID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{
			"error": err.Error(),
		})
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"taskId":   task.TaskID,
		"status":   task.Status,
		"progress": task.Progress,
		"error":    task.Error,
	})
}

// GetResult 获取任务结果
// @author l10840, date 2025-09-18
func (h *Handler) GetResult(c *gin.Context) {
	taskID := c.Param("taskId")
	if taskID == "" {
		c.JSON(http.StatusBadRequest, gin.H{
			"error": "Task ID is required",
		})
		return
	}

	task, err := h.taskManager.GetTask(taskID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{
			"error": err.Error(),
		})
		return
	}

	if task.Status != "completed" {
		c.JSON(http.StatusAccepted, gin.H{
			"taskId": task.TaskID,
			"status": task.Status,
			"error":  task.Error,
		})
		return
	}

	c.JSON(http.StatusOK, task.Result)
}

// ExportResult 导出任务结果
// @author l10840, date 2025-09-18
func (h *Handler) ExportResult(c *gin.Context) {
	taskID := c.Param("taskId")
	format := c.DefaultQuery("format", "json")

	if taskID == "" {
		c.JSON(http.StatusBadRequest, gin.H{
			"error": "Task ID is required",
		})
		return
	}

	result, err := h.taskManager.GetResult(taskID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{
			"error": err.Error(),
		})
		return
	}

	switch format {
	case "json":
		c.Header("Content-Disposition", "attachment; filename=result_"+taskID+".json")
		c.Header("Content-Type", "application/json")
		c.JSON(http.StatusOK, result)
	case "csv":
		h.exportCSV(c, taskID, result)
	default:
		c.JSON(http.StatusBadRequest, gin.H{
			"error": "Unsupported format. Supported: json, csv",
		})
	}
}

// GetTasks 获取所有任务（用于调试）
// @author l10840, date 2025-09-18
func (h *Handler) GetTasks(c *gin.Context) {
	tasks := h.taskManager.GetAllTasks()
	c.JSON(http.StatusOK, tasks)
}

// ExitApp 退出应用程序接口
// @author l10840, date 2025-09-24
func (h *Handler) ExitApp(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{
		"message": "Application exiting...",
	})

	// 异步退出应用，给前端时间接收响应
	go func() {
		systray.Quit()
	}()
}

// Heartbeat 心跳接口，用于保活检测
// @author l10840, date 2025-09-24
func (h *Handler) Heartbeat(c *gin.Context) {
	h.keepAliveManager.UpdateHeartbeat()
	c.JSON(http.StatusOK, gin.H{
		"message": "heartbeat received",
		"time":    time.Now().Format("2006-01-02 15:04:05"),
	})
}

// exportCSV 导出CSV格式
// @author l10840, date 2025-09-18
func (h *Handler) exportCSV(c *gin.Context, taskID string, result *models.TaskResult) {
	c.Header("Content-Disposition", "attachment; filename=result_"+taskID+".csv")
	c.Header("Content-Type", "text/csv")

	// CSV头部
	csvContent := "File Path,Code Added,Code Deleted,Code Modified,Comment Added,Comment Deleted,Comment Modified\n"

	// 文件数据
	for _, file := range result.Files {
		csvContent += fmt.Sprintf("%s,%d,%d,%d,%d,%d,%d\n",
			file.FilePath,
			file.CodeAdded,
			file.CodeDeleted,
			file.CodeModified,
			file.CommentAdded,
			file.CommentDeleted,
			file.CommentModified)
	}

	// 总计行
	csvContent += fmt.Sprintf("\nSummary,%d,%d,%d,%d,%d,%d\n",
		result.Summary.CodeAdded,
		result.Summary.CodeDeleted,
		result.Summary.CodeModified,
		result.Summary.CommentAdded,
		result.Summary.CommentDeleted,
		result.Summary.CommentModified)

	c.String(http.StatusOK, csvContent)
}

// isPathExists 检查路径是否存在
// @author l10840, date 2025-09-18
func isPathExists(path string) bool {
	if path == "" {
		return false
	}

	file, err := os.Open(path)
	if err != nil {
		return false
	}
	defer file.Close()

	stat, err := file.Stat()
	if err != nil {
		return false
	}

	return stat.IsDir()
}