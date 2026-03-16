package main

import (
	"embed"
	"fmt"
	"log"
	"os/exec"
	"runtime"
	"time"

	"code-count/internal/api"
	"code-count/internal/services"
	"github.com/getlantern/systray"
)

//go:embed web/dist/*
var webFS embed.FS

//go:embed icon.ico
var iconData []byte

// @author l10840, date 2025-09-24
func main() {
	systray.Run(onReady, onExit)
}

// 托盘就绪时执行
func onReady() {
	// 设置托盘图标和提示
	if len(iconData) > 0 {
		systray.SetIcon(iconData)
	} else {
		// 使用默认图标数据（简单的像素数据）
		systray.SetIcon(getDefaultIcon())
	}
	systray.SetTitle("Code Count")
	systray.SetTooltip("Code Count Analysis Tool")

	// 创建托盘菜单
	mOpen := systray.AddMenuItem("Open Application", "Open Code Count in browser")
	systray.AddSeparator()
	mQuit := systray.AddMenuItem("Exit", "Exit Code Count")

	// 启动Web服务器
	go startWebServer()

	// 等待服务器启动后打开浏览器
	go func() {
		time.Sleep(2 * time.Second)
		openBrowser("http://localhost:8084")
	}()

	// 监听托盘菜单事件
	go func() {
		for {
			select {
			case <-mOpen.ClickedCh:
				openBrowser("http://localhost:8084")
			case <-mQuit.ClickedCh:
				systray.Quit()
				return
			}
		}
	}()
}

// 托盘退出时执行
func onExit() {
	fmt.Println("Code Count application exiting...")
}

// 启动Web服务器
func startWebServer() {
	fmt.Println("Starting Code Count Analysis Server...")

	// 创建任务管理器和保活管理器
	taskManager := services.NewTaskManager()
	keepAliveManager := services.NewKeepAliveManager()

	// 启动保活监控
	keepAliveManager.Start()

	// 创建路由
	router := api.NewRouter(taskManager, keepAliveManager, webFS)
	engine := router.SetupRoutes()

	// 启动服务器
	port := ":8084"
	fmt.Printf("Server started on http://localhost%s\n", port)
	log.Fatal(engine.Run(port))
}

// 打开浏览器
func openBrowser(url string) {
	var err error

	switch runtime.GOOS {
	case "linux":
		err = exec.Command("xdg-open", url).Start()
	case "windows":
		err = exec.Command("rundll32", "url.dll,FileProtocolHandler", url).Start()
	case "darwin":
		err = exec.Command("open", url).Start()
	default:
		err = fmt.Errorf("unsupported platform")
	}

	if err != nil {
		fmt.Printf("Failed to open browser: %v\n", err)
	}
}

// 获取默认图标（简单的像素数据）
func getDefaultIcon() []byte {
	// 16x16 像素的简单图标数据（可替换为实际的ICO文件）
	return []byte{
		0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
		0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x10,
		0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0xF3, 0xFF, 0x61, 0x00, 0x00, 0x00,
	}
}