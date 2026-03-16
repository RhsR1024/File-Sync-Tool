package models

import (
	"time"
)

// FileStats 单个文件的统计结果
// @author l10840, date 2025-09-18
type FileStats struct {
	FilePath        string `json:"filePath"`
	CodeAdded       int    `json:"codeAdded"`
	CodeDeleted     int    `json:"codeDeleted"`
	CodeModified    int    `json:"codeModified"`
	CommentAdded    int    `json:"commentAdded"`
	CommentDeleted  int    `json:"commentDeleted"`
	CommentModified int    `json:"commentModified"`
}

// Summary 总体统计汇总
// @author l10840, date 2025-09-18
type Summary struct {
	CodeAdded       int `json:"codeAdded"`
	CodeDeleted     int `json:"codeDeleted"`
	CodeModified    int `json:"codeModified"`
	CommentAdded    int `json:"commentAdded"`
	CommentDeleted  int `json:"commentDeleted"`
	CommentModified int `json:"commentModified"`
}

// OperationSummary 操作维度汇总
// @author l10840, date 2025-09-18
type OperationSummary struct {
	AddedTotal    int `json:"addedTotal"`
	DeletedTotal  int `json:"deletedTotal"`
	ModifiedTotal int `json:"modifiedTotal"`
}

// FileTypeSummary 文件类型维度汇总
// @author l10840, date 2025-09-18
type FileTypeSummary map[string]Summary

// TaskResult 任务统计结果
// @author l10840, date 2025-09-18
type TaskResult struct {
	Files            []FileStats      `json:"files"`
	Summary          Summary          `json:"summary"`
	OperationSummary OperationSummary `json:"operationSummary"`
	FileTypeSummary  FileTypeSummary  `json:"fileTypeSummary"`
}

// Progress 任务进度
// @author l10840, date 2025-09-18
type Progress struct {
	Phase          string `json:"phase"`          // scan, diff, stats
	CurrentFile    string `json:"currentFile"`
	ProcessedFiles int    `json:"processedFiles"`
	TotalFiles     int    `json:"totalFiles"`
	Percent        int    `json:"percent"`
}

// Task 任务状态
// @author l10840, date 2025-09-18
type Task struct {
	TaskID     string     `json:"taskId"`
	Status     string     `json:"status"`     // running, completed, failed
	Progress   Progress   `json:"progress"`
	Result     TaskResult `json:"result,omitempty"`
	Error      string     `json:"error,omitempty"`
	StartTime  time.Time  `json:"startTime"`
	EndTime    *time.Time `json:"endTime,omitempty"`
}

// TaskRequest 创建任务请求
// @author l10840, date 2025-09-18
type TaskRequest struct {
	OldPath string `json:"oldPath" binding:"required"`
	NewPath string `json:"newPath" binding:"required"`
}

// TaskResponse 创建任务响应
// @author l10840, date 2025-09-18
type TaskResponse struct {
	TaskID string `json:"taskId"`
}