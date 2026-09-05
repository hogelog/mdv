require_relative "../lib/mdv"

raise "path URL" unless MDV.path_url("/tmp/a b.md") == "/tmp/a%20b.md"
raise "UTF-8 path URL" unless MDV.decode_path(MDV.path_url("/tmp/日本語.md").sub(/^\//, "")) == "tmp/日本語.md"
raise "HTML escaping" unless MDV.escape("<script>&") == "&lt;script&gt;&amp;"
html = MDV.render_markdown("# Heading\n\n- one\n- two\n\n~~old~~ [world](https://example.com).\n\n```\n<script>\n```")
raise "heading" unless html.include?("<h1>Heading</h1>")
raise "list" unless html.include?("<li>one</li>")
raise "strikethrough" unless html.include?("<del>old</del>")
raise "link" unless html.include?("<a href=\"https://example.com\">world</a>")
raise "escaped code" unless html.include?("&lt;script&gt;")
raise "unsafe HTML" if MDV.render_markdown("<script>alert(1)</script>").include?("<script>")
embedded = MDV.embed_images('<img src="assets/screenshot.png" alt="image">', File.expand_path("../README.md", __dir__))
raise "embedded image" unless embedded.include?("src=\"data:image/png;base64,")
document = MDV.render_document("# Design\n\n## 全体の流れ\n\n### Details")
toc = MDV.render_toc("# Design\n\n## 全体の流れ\n\n### Details")
raise "heading ID" unless document.include?("<h2 id=\"全体の流れ\">全体の流れ</h2>")
raise "table of contents" unless toc.include?("href=\"#全体の流れ\"") && toc.include?("toc-level-3")
puts "ok"
