export function colorPlugin(html: string, color: string): string {
    return html.replace(/#{color}/g, color.replace('#', ''));
}
