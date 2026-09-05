require "socket"
require "base64"

module MDVCmark
  ffi_cflags "-I" + File.expand_path("../vendor/cmark-gfm/build/src", __dir__)
  ffi_cflags "-L" + File.expand_path("../vendor/cmark-gfm/build/extensions", __dir__)
  ffi_cflags "-L" + File.expand_path("../vendor/cmark-gfm/build/src", __dir__)
  ffi_lib "cmark-gfm-extensions"
  ffi_lib "cmark-gfm"
  ffi_func :mdv_render_markdown, [:str], :str
end

module MDV
  PORT = 8088
  CLIENT_TIMEOUT = 5
  HELP = "View local Markdown files in your browser.\n\nUsage: mdv [OPTIONS] [FILE]\n\nOptions:\n      --start        Start the background server\n      --stop         Stop it\n  -p, --port PORT    Port for the local server [default: 8088]\n  -h, --help         Print help\n  -V, --version      Print version"
  STYLE = <<~CSS
    :root{color-scheme:light dark;--bg:#fff;--fg:#1f2328;--muted:#656d76;--border:#d1d9e0;--accent:#0969da;--code-bg:#f6f8fa;--quote-bg:#f6f8fa;--th-bg:#f6f8fa;--pre-bg:#1f2328;--pre-fg:#e6edf3;--strong:#0a3069}@media(prefers-color-scheme:dark){:root{--bg:#0d1117;--fg:#e6edf3;--muted:#9198a1;--border:#30363d;--accent:#4493f8;--code-bg:#161b22;--quote-bg:#161b22;--th-bg:#161b22;--pre-bg:#010409;--pre-fg:#e6edf3;--strong:#a5d6ff}}*{box-sizing:border-box}html{scroll-behavior:smooth}body{margin:0;background:var(--bg);color:var(--fg);font-family:-apple-system,BlinkMacSystemFont,"Hiragino Sans","Noto Sans JP","Segoe UI",sans-serif;font-size:15px;line-height:1.75}.wrap{display:flex;max-width:1280px;margin:0 auto;gap:32px}nav.toc{flex:0 0 260px;position:sticky;top:44px;align-self:flex-start;max-height:calc(100vh - 44px);overflow-y:auto;padding:32px 0 32px 20px;font-size:13px;line-height:1.5}nav.toc>.toctitle{font-weight:600;color:var(--muted);text-transform:uppercase;letter-spacing:.06em;font-size:11px;margin-bottom:10px}nav.toc ul{list-style:none;margin:0;padding:0}nav.toc li{margin:3px 0}nav.toc .toc-level-1{font-weight:600}nav.toc .toc-level-2{padding-left:12px}nav.toc .toc-level-3{padding-left:24px;font-size:12px}nav.toc a{color:var(--muted);text-decoration:none;display:block;padding:2px 6px;border-left:2px solid transparent;border-radius:0 4px 4px 0}nav.toc a:hover{color:var(--accent);border-left-color:var(--accent);background:var(--code-bg)}.toc-empty{color:var(--muted)}.document-header{height:44px;display:flex;align-items:center;gap:10px;padding:0 max(20px,calc(50vw - 620px));border-bottom:1px solid color-mix(in srgb,var(--border) 65%,transparent);background:color-mix(in srgb,var(--bg) 94%,transparent);position:sticky;top:0;z-index:10;backdrop-filter:blur(12px)}.document-brand{flex:none;color:var(--muted);font-size:12px;font-weight:600;text-decoration:none;letter-spacing:-.01em}.document-brand:hover{color:var(--fg)}.document-brand::after{content:"/";margin-left:10px;color:color-mix(in srgb,var(--muted) 45%,transparent);font-weight:400}.document-path{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:color-mix(in srgb,var(--muted) 82%,transparent);font:11px ui-monospace,SFMono-Regular,"SF Mono",Menlo,monospace}main{flex:1 1 auto;min-width:0;max-width:900px;margin:0 auto;padding:32px 24px 96px}h1{font-size:28px;line-height:1.4;margin:0 0 8px;padding-bottom:12px;border-bottom:1px solid var(--border);letter-spacing:-.01em}h2{font-size:21px;margin:44px 0 12px;padding-bottom:8px;border-bottom:1px solid var(--border);letter-spacing:-.01em}h3{font-size:17px;margin:28px 0 10px}p{margin:12px 0}a{color:var(--accent)}strong{color:var(--strong);font-weight:600}code{background:var(--code-bg);padding:.15em .4em;border-radius:5px;font-family:ui-monospace,SFMono-Regular,"SF Mono",Menlo,monospace;font-size:.875em;word-break:break-word}pre{background:var(--pre-bg);color:var(--pre-fg);padding:16px 18px;border-radius:8px;overflow-x:auto;line-height:1.5;font-size:12.5px}pre code{background:none;padding:0;color:inherit;font-size:inherit;white-space:pre}blockquote{margin:16px 0;padding:12px 16px;background:var(--quote-bg);border-left:3px solid var(--accent);border-radius:0 6px 6px 0;color:var(--fg)}blockquote p{margin:4px 0}table{border-collapse:collapse;width:100%;margin:16px 0;font-size:13.5px;display:block;overflow-x:auto}th,td{border:1px solid var(--border);padding:8px 12px;text-align:left;vertical-align:top}th{background:var(--th-bg);font-weight:600;white-space:nowrap}tbody tr:nth-child(even){background:color-mix(in srgb,var(--code-bg) 55%,transparent)}ul,ol{padding-left:24px;margin:12px 0}li{margin:5px 0}input[type=checkbox]{margin-right:8px;accent-color:var(--accent)}hr{border:0;border-top:1px solid var(--border);margin:32px 0}img{max-width:100%}.recent{list-style:none;padding:0}.recent li{border-bottom:1px solid var(--border)}.recent a{display:flex;flex-direction:column;padding:14px 4px;text-decoration:none}.recent small{color:var(--muted);overflow-wrap:anywhere}.empty{color:var(--muted);padding:20px 4px}.home-shell{max-width:1040px;margin:0 auto;padding:0 28px}.home-header{height:76px;display:flex;align-items:center;justify-content:space-between;border-bottom:1px solid var(--border)}.brand{display:flex;align-items:center;gap:12px;color:var(--fg);text-decoration:none}.brand-mark{display:grid;place-items:center;width:38px;height:38px;border-radius:9px;background:var(--fg);color:var(--bg);font:700 15px ui-monospace,SFMono-Regular,monospace}.brand strong{display:block;color:var(--fg);font-size:18px;line-height:1.15;letter-spacing:-.02em}.brand small{display:block;color:var(--muted);font-size:11px;line-height:1.4}.home-main{max-width:none;padding:64px 0 80px}.hero{max-width:680px}.eyebrow{margin:0 0 8px;color:var(--accent);font-size:11px;font-weight:700;letter-spacing:.1em;text-transform:uppercase}.hero h1{font-size:38px;border:0;padding:0;margin:0 0 16px;letter-spacing:-.035em}.hero>p:not(.eyebrow){max-width:620px;color:var(--muted);font-size:16px}.hero pre{display:inline-block;margin:18px 0 0;padding:12px 16px}.recent-section{margin-top:72px}.section-heading{display:flex;align-items:flex-end;justify-content:space-between;border-bottom:1px solid var(--border);padding-bottom:10px}.section-heading h2{border:0;margin:0;padding:0;font-size:21px}.section-heading>span{color:var(--muted);font-size:12px}.recent{margin:0}.recent a{padding:16px 4px}.recent strong{color:var(--fg)}.recent a:hover strong{color:var(--accent)}.home-footer{display:flex;gap:24px;flex-wrap:wrap;padding:18px 0 32px;border-top:1px solid var(--border);color:var(--muted);font:11px ui-monospace,SFMono-Regular,monospace}.home-footer a{color:inherit;text-decoration:none}.home-footer a:hover{color:var(--accent);text-decoration:underline}@media(max-width:900px){.wrap{display:block}nav.toc{position:static;max-height:none;padding:24px 24px 18px;flex:none;border-bottom:1px solid var(--border)}main{padding:24px 20px 64px}}@media(max-width:600px){.home-shell{padding:0 20px}.home-header{height:68px}.brand small{display:none}.home-main{padding:44px 0 64px}.hero h1{font-size:31px}.recent-section{margin-top:52px}.home-footer{gap:10px;flex-direction:column}}
  CSS

  def self.config_dir
    root = ENV["XDG_CONFIG_HOME"] || File.join(Dir.home, ".config")
    path = File.join(root, "mdv")
    mkdir_p(path)
    path
  end

  def self.mkdir_p(path)
    missing = []
    current = path
    until Dir.exist?(current)
      missing << current
      parent = File.dirname(current)
      raise "cannot create directory: #{path}" if parent == current
      current = parent
    end
    missing.reverse_each { |directory| Dir.mkdir(directory) unless Dir.exist?(directory) }
  end

  def self.port_file = File.join(config_dir, "daemon.port")
  def self.pid_file = File.join(config_dir, "daemon.pid")
  def self.history_file = File.join(config_dir, "history")
  def self.authorized_file = File.join(config_dir, "authorized")

  def self.escape(value)
    value.gsub("&", "&amp;").gsub("<", "&lt;").gsub(">", "&gt;").gsub('"', "&quot;")
  end

  def self.path_url(path)
    encoded = +""
    path.sub(/^\//, "").bytes.each do |byte|
      character = byte.chr
      if character =~ /[A-Za-z0-9_.~\/-]/
        encoded << character
      else
        encoded << ("%%%02X" % byte)
      end
    end
    "/" + encoded
  end

  def self.decode_path(path)
    path.gsub(/%([0-9A-Fa-f]{2})/) { $1.to_i(16).chr }
  end

  def self.markdown_path(path)
    full = File.realpath(path)
    raise "not a file" unless File.file?(full)
    raise "only Markdown files can be viewed" unless full =~ /\.(md|markdown|mdown)$/i
    full
  end

  def self.authorized
    File.exist?(authorized_file) ? File.readlines(authorized_file, chomp: true) : []
  end

  def self.authorize(path)
    entries = authorized
    return if entries.include?(path)
    File.write(authorized_file, (entries + [path]).join("\n") + "\n")
  end

  def self.add_history(path)
    entries = File.exist?(history_file) ? File.readlines(history_file, chomp: true) : []
    entries.delete(path)
    File.write(history_file, ([path] + entries).first(30).join("\n") + "\n")
  end

  def self.render_markdown(source)
    MDVCmark.mdv_render_markdown(source)
  end

  def self.headings(source)
    ids = Hash.new(0)
    source.each_line.filter_map do |line|
      match = line.match(/^ {0,3}(\#{1,6})\s+(.+?)\s*\#*\s*$/)
      next unless match
      text = match[2].gsub(/[`*_~\[\]]/, "")
      base = text.downcase.gsub(/[^[:alnum:]_-]+/, "-").sub(/^-+/, "").sub(/-+$/, "")
      base = "section" if base.empty?
      count = ids[base]
      ids[base] += 1
      [match[1].length, text, count.zero? ? base : "#{base}-#{count}"]
    end
  end

  def self.render_document(source)
    rendered = render_markdown(source)
    headings(source).each do |level, _text, id|
      rendered = rendered.sub("<h#{level}>", "<h#{level} id=\"#{escape(id)}\">")
    end
    rendered
  end

  def self.image_content_type(path)
    case File.extname(path).downcase
    when ".png" then "image/png"
    when ".jpg", ".jpeg" then "image/jpeg"
    when ".gif" then "image/gif"
    when ".webp" then "image/webp"
    end
  end

  def self.embed_images(html, document)
    directory = File.dirname(document)
    html.gsub(/(<img\b[^>]*\bsrc=")([^"]+)(")/) do
      prefix, source, suffix = $1, $2, $3
      next "#{prefix}#{source}#{suffix}" if source =~ %r{\A(?:[a-z][a-z0-9+.-]*:|/|#)}i
      path = File.realpath(File.expand_path(decode_path(source.split(/[?#]/, 2).first), directory))
      type = image_content_type(path)
      unless type && path.start_with?(directory + "/") && File.file?(path)
        next "#{prefix}#{source}#{suffix}"
      end
      "#{prefix}data:#{type};base64,#{Base64.strict_encode64(File.binread(path))}#{suffix}"
    rescue Errno::ENOENT, ArgumentError
      "#{prefix}#{source}#{suffix}"
    end
  end

  def self.render_toc(source)
    visible = headings(source).select { |level,| level <= 3 }
    return '<p class="toc-empty">No headings</p>' if visible.empty?
    "<ul>#{visible.map { |level, text, id| "<li class=\"toc-level-#{level}\"><a href=\"##{escape(id)}\">#{escape(text)}</a></li>" }.join}</ul>"
  end

  def self.page(title, body)
    <<~HTML
      <!doctype html><html lang="en"><head><meta charset="utf-8">
      <meta name="viewport" content="width=device-width,initial-scale=1">
      <title>#{escape(title)}</title>
      <link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'%3E%3Crect width='64' height='64' rx='14' fill='%231f2328'/%3E%3Ctext x='32' y='41' text-anchor='middle' fill='white' font-family='monospace' font-size='25' font-weight='700'%3EM%E2%86%93%3C/text%3E%3C/svg%3E">
      <link rel="stylesheet" href="/assets/style.css"></head><body>#{body}</body></html>
    HTML
  end

  def self.response(client, status, body, content_type = "text/html; charset=utf-8")
    reason = status == 200 ? "OK" : status == 403 ? "Forbidden" : status == 404 ? "Not Found" : "Bad Request"
    client.write("HTTP/1.1 #{status} #{reason}\r\nContent-Type: #{content_type}\r\nContent-Length: #{body.bytesize}\r\nContent-Security-Policy: default-src 'none'; style-src 'self'; img-src 'self' data:; script-src 'none'; frame-src 'none'; object-src 'none'\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n#{body}")
  end

  def self.index
    entries = File.exist?(history_file) ? File.readlines(history_file, chomp: true) : []
    items = entries.select { |path| File.exist?(path) }.map { |path| "<li><a href=\"#{path_url(path)}\"><strong>#{escape(File.basename(path))}</strong><small>#{escape(path)}</small></a></li>" }.join
    items = '<li class="empty">No recently opened files yet.</li>' if items.empty?
    body = <<~HTML
      <div class="home-shell">
      <header class="home-header"><a class="brand" href="/"><span class="brand-mark">M↓</span><span><strong>mdv</strong><small>Local Markdown Viewer</small></span></a></header>
      <main class="home-main"><section class="hero"><p>Pass a local file to <code>mdv</code>.</p><pre><code>mdv README.md          # start the daemon if needed and open the file
      mdv                    # open the recently-viewed page
      mdv --start            # explicitly start the daemon
      mdv --stop             # stop it
      mdv --start --port 9000</code></pre></section>
      <section class="recent-section"><div class="section-heading"><div><p class="eyebrow">Library</p><h2>Recently opened</h2></div><span>Up to 30 files</span></div><ul class="recent">#{items}</ul></section></main>
      <footer class="home-footer"><span>mdv 0.2.0</span><a href="https://github.com/hogelog/mdv">GitHub</a><span>Data: #{escape(config_dir)}</span></footer></div>
    HTML
    page("mdv — Local Markdown Viewer", body)
  end

  def self.read_request_line(client)
    readable, = IO.select([client], nil, nil, CLIENT_TIMEOUT)
    readable ? client.gets : nil
  end

  def self.handle(client)
    request = read_request_line(client)
    return if request.nil?
    method, target, = request.split(" ")
    while (line = read_request_line(client)) != "\r\n"
      return if line.nil?
    end
    return response(client, 400, page("mdv error", "<main class=\"error\"><h1>400</h1></main>")) unless method == "GET"
    path = target.split("?", 2)[0]
    if path == "/"
      response(client, 200, index)
    elsif path == "/assets/style.css"
      response(client, 200, STYLE, "text/css; charset=utf-8")
    else
      file = File.realpath("/" + decode_path(path.sub(/^\//, "")))
      if file =~ /\.(png|jpe?g|gif|webp|svg)$/i
        allowed = authorized.any? { |document| file.start_with?(File.dirname(document) + "/") }
        return response(client, 403, page("mdv error", "<main class=\"error\"><h1>403</h1><p>image was not opened by mdv</p></main>")) unless allowed
        type = image_content_type(file) || "image/svg+xml"
        response(client, 200, File.binread(file), type)
      else
        file = markdown_path(file)
        return response(client, 403, page("mdv error", "<main class=\"error\"><h1>403</h1><p>file was not opened by mdv</p></main>")) unless authorized.include?(file)
        source = File.read(file)
        body = <<~HTML
          <header class="document-header"><a class="document-brand" href="/">mdv</a><span class="document-path" title="#{escape(file)}">#{escape(file)}</span></header>
          <div class="wrap"><nav class="toc"><div class="toctitle">Contents</div>#{render_toc(source)}</nav><main>#{embed_images(render_document(source), file)}</main></div>
        HTML
        response(client, 200, page(File.basename(file), body))
      end
    end
  rescue Errno::ENOENT, RuntimeError => error
    response(client, 404, page("mdv error", "<main class=\"error\"><h1>404</h1><p>#{escape(error.message)}</p></main>"))
  ensure
    client.close unless client.nil?
  end

  def self.serve(port)
    server = nil
    actual = port
    loop do
      begin
        server = TCPServer.new("127.0.0.1", actual)
        break
      rescue Errno::EADDRINUSE
        actual += 1
      end
    end
    File.write(pid_file, Process.pid.to_s)
    File.write(port_file, actual.to_s)
    puts "mdv daemon listening at http://localhost:#{actual}"
    # Browsers may open a speculative connection before sending its request.
    # Do not let that idle socket prevent the listener from accepting the
    # connection that carries the actual document request.
    loop do
      client = server.accept
      Thread.new(client) { |connection| handle(connection) }
    end
  ensure
    File.delete(pid_file) if File.exist?(pid_file)
    File.delete(port_file) if File.exist?(port_file)
  end

  def self.running_port
    return nil unless File.exist?(port_file)
    port = File.read(port_file).to_i
    begin
      socket = TCPSocket.new("127.0.0.1", port)
      socket.close
      port
    rescue Errno::ECONNREFUSED
      nil
    end
  end

  def self.ensure_daemon(port)
    existing = running_port
    return existing unless existing.nil?
    File.delete(port_file) if File.exist?(port_file)
    Process.spawn($0, "--daemon", "--port", port.to_s, out: File::NULL, err: File::NULL)
    30.times do
      sleep 0.05
      existing = running_port
      return existing unless existing.nil?
    end
    raise "daemon did not start"
  end

  def self.stop
    raise "no mdv daemon is running" unless File.exist?(pid_file)
    Process.kill("TERM", File.read(pid_file).to_i)
    File.delete(pid_file) if File.exist?(pid_file)
    File.delete(port_file) if File.exist?(port_file)
  end

  def self.parse_port(value)
    port = value.to_i
    raise "invalid port: #{value}" unless value =~ /^\d+$/ && port > 0 && port <= 65_535
    port
  end

  def self.open_url(url)
    return if ENV["MDV_NO_OPEN"] == "1"
    command = RUBY_PLATFORM =~ /darwin/ ? "open" : "xdg-open"
    Process.spawn(command, url, out: File::NULL, err: File::NULL)
  end

  def self.main(argv)
    port = PORT
    start = false
    stop_requested = false
    daemon = false
    file = nil
    index = 0
    while index < argv.length
      arg = argv[index]
      if arg == "--start"
        start = true
      elsif arg == "--stop"
        stop_requested = true
      elsif arg == "--daemon"
        daemon = true
      elsif arg == "-p" || arg == "--port"
        index += 1
        raise "#{arg} requires a port" if index >= argv.length
        port = parse_port(argv[index])
      elsif arg.start_with?("--port=")
        port = parse_port(arg.sub("--port=", ""))
      elsif arg == "-h" || arg == "--help"
        puts HELP
        return
      elsif arg == "-V" || arg == "--version"
        puts "mdv 0.2.0"
        return
      elsif arg.start_with?("-")
        raise "unknown option: #{arg}"
      elsif file.nil?
        file = arg
      else
        raise "only one Markdown file can be specified"
      end
      index += 1
    end
    raise "--start and --stop cannot be used together" if start && stop_requested
    raise "a file cannot be combined with --start or --stop" if !file.nil? && (start || stop_requested)
    return serve(port) if daemon
    if stop_requested
      stop
      puts "mdv daemon stopped"
      return
    end
    actual = ensure_daemon(port)
    if start
      puts "mdv daemon listening at http://localhost:#{actual}"
      return
    end
    if file.nil?
      url = "http://localhost:#{actual}"
    else
      full = markdown_path(file)
      authorize(full)
      add_history(full)
      url = "http://localhost:#{actual}#{path_url(full)}"
    end
    open_url(url)
    puts url
  end
end
