// Common/base styles shared across ALL themes
// Extracted from theme.js compiled output and global.less
export const commonCss = `
.rs-view *,
.rs-view *::before,
.rs-view *::after {
  box-sizing: border-box;
}
.rs-view {
  width: 100%;
  display: block;
  background: #fff;
  -webkit-text-size-adjust: none;
}
.rs-view h1, .rs-view h2, .rs-view h3,
.rs-view h4, .rs-view h5, .rs-view h6,
.rs-view p, .rs-view ul, .rs-view ol,
.rs-view li, .rs-view blockquote, .rs-view pre {
  margin: 0;
  padding: 0;
}
.rs-view ul, .rs-view ol {
  list-style: none;
}
.rs-view .lr-container {
  display: flex;
  padding: 10px 0 20px;
  justify-content: space-between;
}
.rs-view .left {
  flex: auto;
  text-align: left;
}
.rs-view .right {
  flex: auto;
  text-align: right;
}
.rs-view a {
  color: #d4d4d4;
  text-decoration: none;
  background-color: transparent;
  outline: none;
  cursor: pointer;
  -webkit-transition: color 0.3s;
  transition: color 0.3s;
  -webkit-text-decoration-skip: objects;
}
.rs-view img {
  max-width: 400px;
}
.rs-view code {
  padding: 3px 6px;
  display: inline-block;
  color: #333;
  border-radius: 4px;
  margin: 0 2px 0px;
  background-color: rgba(27, 31, 35, 0.05);
  word-break: break-all;
  font-size: 13px;
}
.rs-view h1 {
  font-weight: 900;
}
.rs-view h2 {
  font-weight: 700;
}
.rs-view h3 {
  font-weight: 600;
}
.rs-view h4,
.rs-view h5 {
  position: relative;
  line-height: 20px;
  overflow: hidden;
  margin-left: 50px;
  margin-right: 50px;
  font-size: 13px;
  font-weight: 500;
}
.rs-view .h2_block + .h2_block {
  padding-top: 10px;
}
.rs-view .h3_block + .h3_block {
  padding-top: 0px;
}
.rs-view ul {
  overflow: hidden;
}
.rs-view ul li {
  list-style: square inside;
}
.rs-view li ul li {
  padding-left: 1px;
  list-style: circle inside;
}
.rs-view ol {
  overflow: hidden;
  margin: 0 50px;
}
.rs-view ol li {
  list-style: decimal inside;
}
.rs-view ol ul li {
  padding-left: 1px;
  list-style: circle inside;
}
.break-line {
  display: block;
  width: 100%;
  height: 5px;
}
.rs-view .h1_block a {
  display: inline;
}
.rs-view .h1_block p {
  line-height: 1.6;
}
.rs-view .icon {
  display: inline-block;
  fill: currentColor;
  width: 16px;
  height: 16px;
  vertical-align: middle;
  margin-right: 4px;
}
.rs-view svg.icon {
  display: inline-block;
  fill: currentColor;
  width: 16px;
  height: 16px;
  vertical-align: middle;
  margin-right: 4px;
}
`;
