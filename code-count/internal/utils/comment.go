package utils

import (
	"path/filepath"
	"strings"
)

// CommentRule 注释规则
// @author l10840, date 2025-09-18
type CommentRule struct {
	SingleLine  []string
	MultiStart  []string
	MultiEnd    []string
}

// GetCommentRules 获取各语言的注释规则
// @author l10840, date 2025-09-18
func GetCommentRules() map[string]CommentRule {
	return map[string]CommentRule{
		".go":   {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".java": {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".c":    {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".cpp":  {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".cc":   {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".js":   {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".ts":   {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".py":   {SingleLine: []string{"#"}, MultiStart: []string{}, MultiEnd: []string{}},
		".sh":   {SingleLine: []string{"#"}, MultiStart: []string{}, MultiEnd: []string{}},
		".sql":  {SingleLine: []string{"--"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".xml":  {SingleLine: []string{}, MultiStart: []string{"<!--"}, MultiEnd: []string{"-->"}},
		".html": {SingleLine: []string{}, MultiStart: []string{"<!--"}, MultiEnd: []string{"-->"}},
		".css":  {SingleLine: []string{}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".scss": {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".less": {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".php":  {SingleLine: []string{"//", "#"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".rb":   {SingleLine: []string{"#"}, MultiStart: []string{"=begin"}, MultiEnd: []string{"=end"}},
		".pl":   {SingleLine: []string{"#"}, MultiStart: []string{}, MultiEnd: []string{}},
		".lua":  {SingleLine: []string{"--"}, MultiStart: []string{"--[["}, MultiEnd: []string{"]]"}},
		".r":    {SingleLine: []string{"#"}, MultiStart: []string{}, MultiEnd: []string{}},
		".swift": {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".kt":   {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".dart": {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".rs":   {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
		".scala": {SingleLine: []string{"//"}, MultiStart: []string{"/*"}, MultiEnd: []string{"*/"}},
	}
}

// IsComment 判断一行是否为注释
// @author l10840, date 2025-09-18
func IsComment(line, fileExt string) bool {
	rules := GetCommentRules()
	rule, exists := rules[fileExt]
	if !exists {
		return false
	}

	trimmed := strings.TrimSpace(line)
	if trimmed == "" {
		return false
	}

	// 检查单行注释
	for _, prefix := range rule.SingleLine {
		if strings.HasPrefix(trimmed, prefix) {
			return true
		}
	}

	// 检查多行注释开始
	for _, prefix := range rule.MultiStart {
		if strings.HasPrefix(trimmed, prefix) {
			return true
		}
	}

	return false
}

// GetFileExtension 获取文件扩展名
// @author l10840, date 2025-09-18
func GetFileExtension(filename string) string {
	return strings.ToLower(filepath.Ext(filename))
}

// IsSupportedFile 判断是否为支持的文件类型
// @author l10840, date 2025-09-18
func IsSupportedFile(filename string) bool {
	ext := GetFileExtension(filename)
	rules := GetCommentRules()
	_, exists := rules[ext]
	return exists
}

// IsEmptyOrWhitespace 判断行是否为空或仅包含空白字符
// @author l10840, date 2025-09-18
func IsEmptyOrWhitespace(line string) bool {
	return strings.TrimSpace(line) == ""
}