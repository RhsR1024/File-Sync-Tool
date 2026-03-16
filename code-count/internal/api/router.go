package api

import (
	"code-count/internal/services"
	"embed"
	"io/fs"
	"net/http"

	"github.com/gin-gonic/gin"
)

// Router 路由配置
// @author l10840, date 2025-09-18
type Router struct {
	handler *Handler
	webFS   embed.FS
}

// NewRouter 创建路由
// @author l10840, date 2025-09-18
func NewRouter(taskManager *services.TaskManager, keepAliveManager *services.KeepAliveManager, webFS embed.FS) *Router {
	return &Router{
		handler: NewHandler(taskManager, keepAliveManager),
		webFS:   webFS,
	}
}

// SetupRoutes 设置路由
// @author l10840, date 2025-09-18
func (r *Router) SetupRoutes() *gin.Engine {
	gin.SetMode(gin.ReleaseMode)
	router := gin.Default()

	// 跨域中间件
	router.Use(func(c *gin.Context) {
		c.Header("Access-Control-Allow-Origin", "*")
		c.Header("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS")
		c.Header("Access-Control-Allow-Headers", "Content-Type, Authorization")

		if c.Request.Method == "OPTIONS" {
			c.AbortWithStatus(http.StatusNoContent)
			return
		}

		c.Next()
	})

	// API路由组
	api := router.Group("/api")
	{
		api.POST("/tasks", r.handler.CreateTask)
		api.GET("/progress/:taskId", r.handler.GetProgress)
		api.GET("/results/:taskId", r.handler.GetResult)
		api.GET("/results/:taskId/export", r.handler.ExportResult)
		api.GET("/tasks", r.handler.GetTasks) // 调试用
		api.POST("/exit", r.handler.ExitApp)  // 退出应用
		api.POST("/heartbeat", r.handler.Heartbeat) // 心跳保活
	}

	// 静态文件服务
	r.setupStaticFiles(router)

	return router
}

// setupStaticFiles 设置静态文件服务
// @author l10840, date 2025-09-18
func (r *Router) setupStaticFiles(router *gin.Engine) {
	// 尝试获取嵌入的静态文件
	distFS, err := fs.Sub(r.webFS, "web/dist")
	if err == nil {
		// 如果有嵌入的文件，使用嵌入的文件系统
		router.StaticFS("/static", http.FS(distFS))

		// 主页路由
		router.GET("/", func(c *gin.Context) {
			file, err := distFS.Open("index.html")
			if err != nil {
				c.String(http.StatusNotFound, "Frontend not available")
				return
			}
			defer file.Close()

			stat, err := file.Stat()
			if err != nil {
				c.String(http.StatusNotFound, "Frontend not available")
				return
			}

			c.DataFromReader(http.StatusOK, stat.Size(), "text/html", file, nil)
		})
	} else {
		// 如果没有嵌入文件，提供简单的HTML界面
		router.GET("/", r.serveFallbackHTML)
		router.GET("/static/*filepath", r.serveFallbackHTML)
	}
}

// serveFallbackHTML 提供备用HTML界面
// @author l10840, date 2025-09-18
func (r *Router) serveFallbackHTML(c *gin.Context) {
	html := `
<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>代码修改统计系统</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; }
        .container { max-width: 800px; margin: 0 auto; }
        .form-group { margin: 15px 0; }
        label { display: block; margin-bottom: 5px; }
        input[type="text"] { width: 100%; padding: 8px; }
        button { padding: 10px 20px; background: #007bff; color: white; border: none; cursor: pointer; }
        button:hover { background: #0056b3; }
        .progress { width: 100%; height: 20px; background: #f0f0f0; margin: 10px 0; }
        .progress-bar { height: 100%; background: #007bff; transition: width 0.3s; }
        .result { margin-top: 20px; }
        table { border-collapse: collapse; width: 100%; }
        th, td { border: 1px solid #ddd; padding: 8px; text-align: left; }
        th { background-color: #f2f2f2; }
        #loading { display: none; }
    </style>
</head>
<body>
    <div class="container">
        <h1>代码修改统计系统</h1>

        <div class="form-group">
            <label for="oldPath">旧版本路径:</label>
            <input type="text" id="oldPath" placeholder="例如: C:\\old-code">
        </div>

        <div class="form-group">
            <label for="newPath">新版本路径:</label>
            <input type="text" id="newPath" placeholder="例如: C:\\new-code">
        </div>

        <button onclick="startAnalysis()">开始分析</button>
        <button onclick="exitApp()" style="background: #dc3545; margin-left: 10px;">退出程序</button>

        <div id="loading">
            <h3>分析进度</h3>
            <div class="progress">
                <div class="progress-bar" id="progressBar" style="width: 0%"></div>
            </div>
            <p id="progressText">准备中...</p>
        </div>

        <div class="result" id="result" style="display: none;">
            <h3>分析结果</h3>
            <div id="summary"></div>
            <table id="fileTable">
                <thead>
                    <tr>
                        <th>文件路径</th>
                        <th>新增代码</th>
                        <th>删除代码</th>
                        <th>修改代码</th>
                        <th>新增注释</th>
                        <th>删除注释</th>
                        <th>修改注释</th>
                    </tr>
                </thead>
                <tbody id="fileTableBody">
                </tbody>
            </table>
        </div>
    </div>

    <script>
        let currentTaskId = null;
        let progressTimer = null;

        async function startAnalysis() {
            const oldPath = document.getElementById('oldPath').value;
            const newPath = document.getElementById('newPath').value;

            if (!oldPath || !newPath) {
                alert('请输入两个路径');
                return;
            }

            try {
                const response = await fetch('/api/tasks', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ oldPath, newPath })
                });

                const data = await response.json();
                if (response.ok) {
                    currentTaskId = data.taskId;
                    document.getElementById('loading').style.display = 'block';
                    document.getElementById('result').style.display = 'none';
                    startProgressMonitoring();
                } else {
                    alert('错误: ' + data.error);
                }
            } catch (error) {
                alert('请求失败: ' + error.message);
            }
        }

        async function startProgressMonitoring() {
            progressTimer = setInterval(async () => {
                try {
                    const response = await fetch('/api/progress/' + currentTaskId);
                    const data = await response.json();

                    if (response.ok) {
                        updateProgress(data);

                        if (data.status === 'completed') {
                            clearInterval(progressTimer);
                            await loadResult();
                        } else if (data.status === 'failed') {
                            clearInterval(progressTimer);
                            alert('分析失败: ' + data.error);
                            document.getElementById('loading').style.display = 'none';
                        }
                    }
                } catch (error) {
                    console.error('获取进度失败:', error);
                }
            }, 1000);
        }

        function updateProgress(data) {
            const progress = data.progress;
            const percent = progress.percent || 0;

            document.getElementById('progressBar').style.width = percent + '%';
            document.getElementById('progressText').textContent =
                progress.phase + ': ' + progress.currentFile +
                ' (' + progress.processedFiles + '/' + progress.totalFiles + ')';
        }

        async function loadResult() {
            try {
                const response = await fetch('/api/results/' + currentTaskId);
                const data = await response.json();

                if (response.ok) {
                    displayResult(data);
                    document.getElementById('loading').style.display = 'none';
                    document.getElementById('result').style.display = 'block';
                } else {
                    alert('获取结果失败: ' + data.error);
                }
            } catch (error) {
                alert('获取结果失败: ' + error.message);
            }
        }

        function displayResult(data) {
            // 显示汇总信息
            const summary = data.summary;
            document.getElementById('summary').innerHTML =
                '<p><strong>总体统计:</strong></p>' +
                '<p>代码: 新增 ' + summary.codeAdded + ', 删除 ' + summary.codeDeleted + ', 修改 ' + summary.codeModified + '</p>' +
                '<p>注释: 新增 ' + summary.commentAdded + ', 删除 ' + summary.commentDeleted + ', 修改 ' + summary.commentModified + '</p>';

            // 显示文件列表
            const tbody = document.getElementById('fileTableBody');
            tbody.innerHTML = '';

            data.files.forEach(file => {
                const row = tbody.insertRow();
                row.insertCell(0).textContent = file.filePath;
                row.insertCell(1).textContent = file.codeAdded;
                row.insertCell(2).textContent = file.codeDeleted;
                row.insertCell(3).textContent = file.codeModified;
                row.insertCell(4).textContent = file.commentAdded;
                row.insertCell(5).textContent = file.commentDeleted;
                row.insertCell(6).textContent = file.commentModified;
            });
        }

        async function exitApp() {
            if (confirm('确定要退出程序吗？')) {
                try {
                    await fetch('/api/exit', { method: 'POST' });
                    window.close(); // 尝试关闭当前标签页
                } catch (error) {
                    console.log('Exit request sent');
                    window.close();
                }
            }
        }

        // 监听页面关闭事件，自动退出应用
        window.addEventListener('beforeunload', function(e) {
            // 页面关闭时停止心跳，让服务端自然退出
            if (heartbeatTimer) {
                clearInterval(heartbeatTimer);
            }
        });

        // 启动心跳机制
        let heartbeatTimer = null;
        function startHeartbeat() {
            // 立即发送一次心跳
            sendHeartbeat();

            // 每30秒发送一次心跳
            heartbeatTimer = setInterval(sendHeartbeat, 30000);
        }

        function sendHeartbeat() {
            fetch('/api/heartbeat', { method: 'POST' }).catch(error => {
                console.log('Heartbeat failed:', error);
            });
        }

        // 页面加载完成后启动心跳
        document.addEventListener('DOMContentLoaded', function() {
            startHeartbeat();
        });

        // 如果页面已经加载完成，立即启动心跳
        if (document.readyState === 'loading') {
            document.addEventListener('DOMContentLoaded', startHeartbeat);
        } else {
            startHeartbeat();
        }
    </script>
</body>
</html>
    `
	c.Data(http.StatusOK, "text/html", []byte(html))
}