package services

import (
	"bufio"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"code-count/internal/models"
	"code-count/internal/utils"
)

// FileScanner 文件扫描器
// @author l10840, date 2025-09-18
type FileScanner struct {
	progressCallback func(phase string, currentFile string, processed, total int)
}

// NewFileScanner 创建文件扫描器
// @author l10840, date 2025-09-18
func NewFileScanner(progressCallback func(string, string, int, int)) *FileScanner {
	return &FileScanner{
		progressCallback: progressCallback,
	}
}

// FileInfo 文件信息
// @author l10840, date 2025-09-18
type FileInfo struct {
	Path         string
	RelativePath string
	Content      []string
}

// ScanFiles 扫描目录中的支持文件
// @author l10840, date 2025-09-18
func (fs *FileScanner) ScanFiles(rootPath string) (map[string]*FileInfo, error) {
	files := make(map[string]*FileInfo)

	err := filepath.Walk(rootPath, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		if info.IsDir() {
			return nil
		}

		// 检查是否为支持的文件类型
		if !utils.IsSupportedFile(info.Name()) {
			return nil
		}

		// 获取相对路径
		relPath, err := filepath.Rel(rootPath, path)
		if err != nil {
			return err
		}

		// 读取文件内容
		content, err := fs.readFileLines(path)
		if err != nil {
			fmt.Printf("Warning: failed to read file %s: %v\n", path, err)
			return nil // 忽略读取失败的文件，继续处理其他文件
		}

		files[relPath] = &FileInfo{
			Path:         path,
			RelativePath: relPath,
			Content:      content,
		}

		return nil
	})

	return files, err
}

// readFileLines 读取文件所有行
// @author l10840, date 2025-09-18
func (fs *FileScanner) readFileLines(filePath string) ([]string, error) {
	file, err := os.Open(filePath)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	var lines []string
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		lines = append(lines, scanner.Text())
	}

	return lines, scanner.Err()
}

// CompareDirectories 对比两个目录
// @author l10840, date 2025-09-18
func (fs *FileScanner) CompareDirectories(oldPath, newPath string) (*models.TaskResult, error) {
	// 扫描旧目录
	if fs.progressCallback != nil {
		fs.progressCallback("scan", "Scanning old directory...", 0, 1)
	}
	oldFiles, err := fs.ScanFiles(oldPath)
	if err != nil {
		return nil, fmt.Errorf("failed to scan old directory: %w", err)
	}

	// 扫描新目录
	if fs.progressCallback != nil {
		fs.progressCallback("scan", "Scanning new directory...", 1, 2)
	}
	newFiles, err := fs.ScanFiles(newPath)
	if err != nil {
		return nil, fmt.Errorf("failed to scan new directory: %w", err)
	}

	// 获取所有文件路径的并集
	allFiles := make(map[string]bool)
	for path := range oldFiles {
		allFiles[path] = true
	}
	for path := range newFiles {
		allFiles[path] = true
	}

	result := &models.TaskResult{
		Files:           make([]models.FileStats, 0),
		FileTypeSummary: make(models.FileTypeSummary),
	}

	totalFiles := len(allFiles)
	processed := 0

	// 对每个文件进行diff对比
	for filePath := range allFiles {
		if fs.progressCallback != nil {
			fs.progressCallback("diff", filePath, processed, totalFiles)
		}

		oldContent := []string{}
		newContent := []string{}

		if oldFile, exists := oldFiles[filePath]; exists {
			oldContent = oldFile.Content
		}
		if newFile, exists := newFiles[filePath]; exists {
			newContent = newFile.Content
		}

		// 计算文件统计
		stats := fs.calculateFileStats(filePath, oldContent, newContent)

		// 只添加有变更的文件到结果中，并且只统计有变更的文件
		if fs.hasChanges(stats) {
			result.Files = append(result.Files, stats)

			// 更新总体统计（只包括有变更的文件）
			result.Summary.CodeAdded += stats.CodeAdded
			result.Summary.CodeDeleted += stats.CodeDeleted
			result.Summary.CodeModified += stats.CodeModified
			result.Summary.CommentAdded += stats.CommentAdded
			result.Summary.CommentDeleted += stats.CommentDeleted
			result.Summary.CommentModified += stats.CommentModified

			// 更新文件类型统计（只包括有变更的文件格式）
			ext := utils.GetFileExtension(filePath)
			if _, exists := result.FileTypeSummary[ext]; !exists {
				result.FileTypeSummary[ext] = models.Summary{}
			}
			extSummary := result.FileTypeSummary[ext]
			extSummary.CodeAdded += stats.CodeAdded
			extSummary.CodeDeleted += stats.CodeDeleted
			extSummary.CodeModified += stats.CodeModified
			extSummary.CommentAdded += stats.CommentAdded
			extSummary.CommentDeleted += stats.CommentDeleted
			extSummary.CommentModified += stats.CommentModified
			result.FileTypeSummary[ext] = extSummary
		}

		processed++
	}

	// 计算操作维度统计
	result.OperationSummary.AddedTotal = result.Summary.CodeAdded + result.Summary.CommentAdded
	result.OperationSummary.DeletedTotal = result.Summary.CodeDeleted + result.Summary.CommentDeleted
	result.OperationSummary.ModifiedTotal = result.Summary.CodeModified + result.Summary.CommentModified

	if fs.progressCallback != nil {
		fs.progressCallback("completed", "Analysis completed", totalFiles, totalFiles)
	}

	return result, nil
}

// calculateFileStats 计算单个文件的统计数据
// @author l10840, date 2025-09-18
func (fs *FileScanner) calculateFileStats(filePath string, oldContent, newContent []string) models.FileStats {
	stats := models.FileStats{
		FilePath: filePath,
	}

	fileExt := utils.GetFileExtension(filePath)

	// 简单的行级对比算法
	oldLines := fs.preprocessLines(oldContent)
	newLines := fs.preprocessLines(newContent)

	// 创建行内容到行号的映射
	oldLineMap := make(map[string][]int)
	newLineMap := make(map[string][]int)

	for i, line := range oldLines {
		if line != "" {
			oldLineMap[line] = append(oldLineMap[line], i)
		}
	}
	for i, line := range newLines {
		if line != "" {
			newLineMap[line] = append(newLineMap[line], i)
		}
	}

	// 标记已匹配的行
	oldMatched := make([]bool, len(oldLines))
	newMatched := make([]bool, len(newLines))

	// 找到完全匹配的行
	for content, oldIndices := range oldLineMap {
		if newIndices, exists := newLineMap[content]; exists {
			// 简单匹配策略：按顺序匹配
			minLen := len(oldIndices)
			if len(newIndices) < minLen {
				minLen = len(newIndices)
			}
			for i := 0; i < minLen; i++ {
				oldMatched[oldIndices[i]] = true
				newMatched[newIndices[i]] = true
			}
		}
	}

	// 统计未匹配的行
	for i, line := range oldLines {
		if !oldMatched[i] && line != "" {
			if utils.IsComment(oldContent[i], fileExt) {
				stats.CommentDeleted++
			} else {
				stats.CodeDeleted++
			}
		}
	}

	for i, line := range newLines {
		if !newMatched[i] && line != "" {
			if utils.IsComment(newContent[i], fileExt) {
				stats.CommentAdded++
			} else {
				stats.CodeAdded++
			}
		}
	}

	return stats
}

// preprocessLines 预处理行内容，去除空行和仅包含空白字符的行
// @author l10840, date 2025-09-18
func (fs *FileScanner) preprocessLines(lines []string) []string {
	var result []string
	for _, line := range lines {
		trimmed := strings.TrimSpace(line)
		result = append(result, trimmed)
	}
	return result
}

// hasChanges 判断文件是否有变更
// @author l10840, date 2025-09-18
func (fs *FileScanner) hasChanges(stats models.FileStats) bool {
	return stats.CodeAdded > 0 || stats.CodeDeleted > 0 || stats.CodeModified > 0 ||
		   stats.CommentAdded > 0 || stats.CommentDeleted > 0 || stats.CommentModified > 0
}