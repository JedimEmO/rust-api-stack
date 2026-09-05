        function appendInlineMarkdown(parent, text) {
            let index = 0;
            const source = String(text || "");

            while (index < source.length) {
                if (source.startsWith("**", index)) {
                    const end = source.indexOf("**", index + 2);
                    if (end > index + 2) {
                        const strong = document.createElement("strong");
                        appendInlineMarkdown(strong, source.slice(index + 2, end));
                        parent.appendChild(strong);
                        index = end + 2;
                        continue;
                    }
                }

                if (source[index] === "`") {
                    const end = source.indexOf("`", index + 1);
                    if (end > index + 1) {
                        const code = document.createElement("code");
                        code.textContent = source.slice(index + 1, end);
                        parent.appendChild(code);
                        index = end + 1;
                        continue;
                    }
                }

                if (source[index] === "[") {
                    const labelEnd = source.indexOf("]", index + 1);
                    const urlStart = labelEnd + 1;
                    if (labelEnd > index + 1 && source[urlStart] === "(") {
                        const urlEnd = source.indexOf(")", urlStart + 1);
                        const href = source.slice(urlStart + 1, urlEnd);
                        if (urlEnd > urlStart + 1 && isSafeMarkdownUrl(href)) {
                            const link = document.createElement("a");
                            link.href = href;
                            link.target = "_blank";
                            link.rel = "noreferrer noopener";
                            appendInlineMarkdown(link, source.slice(index + 1, labelEnd));
                            parent.appendChild(link);
                            index = urlEnd + 1;
                            continue;
                        }
                    }
                }

                const next = ["**", "`", "["]
                    .map((token) => source.indexOf(token, index + 1))
                    .filter((position) => position !== -1)
                    .sort((a, b) => a - b)[0] ?? source.length;
                parent.appendChild(document.createTextNode(source.slice(index, next)));
                index = next;
            }
        }

        function isSafeMarkdownUrl(href) {
            try {
                const url = new URL(href, window.location.href);
                return url.protocol === "http:" || url.protocol === "https:";
            } catch (_) {
                return false;
            }
        }

        function renderMarkdownInto(container, text) {
            container.textContent = "";
            container.classList.add("markdown");

            const lines = String(text || "").replace(/\r\n?/g, "\n").split("\n");
            let index = 0;

            while (index < lines.length) {
                if (!lines[index].trim()) {
                    index += 1;
                    continue;
                }

                if (lines[index].trimStart().startsWith("```")) {
                    const codeLines = [];
                    index += 1;
                    while (index < lines.length && !lines[index].trimStart().startsWith("```")) {
                        codeLines.push(lines[index]);
                        index += 1;
                    }
                    if (index < lines.length) index += 1;

                    const pre = document.createElement("pre");
                    const code = document.createElement("code");
                    code.textContent = codeLines.join("\n");
                    pre.appendChild(code);
                    container.appendChild(pre);
                    continue;
                }

                if (/^\s*-\s+/.test(lines[index])) {
                    const list = document.createElement("ul");
                    while (index < lines.length && /^\s*-\s+/.test(lines[index])) {
                        const item = document.createElement("li");
                        appendInlineMarkdown(item, lines[index].replace(/^\s*-\s+/, ""));
                        list.appendChild(item);
                        index += 1;
                    }
                    container.appendChild(list);
                    continue;
                }

                const paragraphLines = [];
                while (
                    index < lines.length
                    && lines[index].trim()
                    && !lines[index].trimStart().startsWith("```")
                    && !/^\s*-\s+/.test(lines[index])
                ) {
                    paragraphLines.push(lines[index]);
                    index += 1;
                }
                const paragraph = document.createElement("p");
                appendInlineMarkdown(paragraph, paragraphLines.join("\n"));
                container.appendChild(paragraph);
            }
        }

