//! Conversation context management. Tracks messages, handles compaction.

use kith_common::inference::{Message, MessageContent, Role};

pub struct ConversationContext {
    messages: Vec<Message>,
    max_messages: usize,
}

impl ConversationContext {
    pub fn new(max_messages: usize) -> Self {
        Self {
            messages: Vec::new(),
            max_messages,
        }
    }

    /// Add the system prompt as the first message.
    pub fn set_system_prompt(&mut self, prompt: String) {
        // Replace existing system message or insert at beginning
        if let Some(first) = self.messages.first()
            && matches!(first.role, Role::System) {
                self.messages[0] = Message {
                    role: Role::System,
                    content: MessageContent::Text(prompt),
                };
                return;
            }
        self.messages.insert(
            0,
            Message {
                role: Role::System,
                content: MessageContent::Text(prompt),
            },
        );
    }

    /// Add a user message.
    pub fn add_user(&mut self, text: String) {
        self.messages.push(Message {
            role: Role::User,
            content: MessageContent::Text(text),
        });
        self.compact_if_needed();
    }

    /// Add an assistant message.
    pub fn add_assistant(&mut self, content: MessageContent) {
        self.messages.push(Message {
            role: Role::Assistant,
            content,
        });
        self.compact_if_needed();
    }

    /// Add a tool result.
    pub fn add_tool_result(&mut self, tool_call_id: String, output: String) {
        self.messages.push(Message {
            role: Role::Tool { tool_call_id },
            content: MessageContent::Text(output),
        });
    }

    /// Get all messages for sending to the backend.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Number of messages (including system).
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Simple compaction: keep system prompt + last N messages.
    fn compact_if_needed(&mut self) {
        if self.messages.len() <= self.max_messages {
            return;
        }

        let has_system = matches!(self.messages.first().map(|m| &m.role), Some(Role::System));

        if has_system && self.messages.len() > self.max_messages {
            // Keep system prompt + tail
            let keep = self.max_messages - 1;
            let start = self.messages.len() - keep;
            let system = self.messages[0].clone();
            let tail: Vec<Message> = self.messages[start..].to_vec();
            self.messages = vec![system];
            self.messages.extend(tail);
        }
    }

    /// Reset conversation (keep system prompt if present).
    pub fn reset(&mut self) {
        let system = if matches!(self.messages.first().map(|m| &m.role), Some(Role::System)) {
            Some(self.messages[0].clone())
        } else {
            None
        };

        self.messages.clear();
        if let Some(sys) = system {
            self.messages.push(sys);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kith_common::inference::ToolCall;

    #[test]
    fn new_context_is_empty() {
        let ctx = ConversationContext::new(100);
        assert!(ctx.is_empty());
    }

    #[test]
    fn system_prompt_set() {
        let mut ctx = ConversationContext::new(100);
        ctx.set_system_prompt("you are a shell".into());
        assert_eq!(ctx.len(), 1);
        assert!(matches!(ctx.messages()[0].role, Role::System));
    }

    #[test]
    fn system_prompt_replaced_not_duplicated() {
        let mut ctx = ConversationContext::new(100);
        ctx.set_system_prompt("prompt v1".into());
        ctx.set_system_prompt("prompt v2".into());
        assert_eq!(ctx.len(), 1);
        assert!(matches!(&ctx.messages()[0].content, MessageContent::Text(t) if t == "prompt v2"));
    }

    #[test]
    fn user_and_assistant_messages() {
        let mut ctx = ConversationContext::new(100);
        ctx.set_system_prompt("sys".into());
        ctx.add_user("hello".into());
        ctx.add_assistant(MessageContent::Text("hi".into()));
        assert_eq!(ctx.len(), 3);
    }

    #[test]
    fn tool_result_added() {
        let mut ctx = ConversationContext::new(100);
        ctx.add_assistant(MessageContent::ToolCalls(vec![ToolCall {
            id: "call_1".into(),
            name: "remote".into(),
            arguments: serde_json::json!({}),
        }]));
        ctx.add_tool_result("call_1".into(), "output".into());
        assert_eq!(ctx.len(), 2);
        assert!(matches!(ctx.messages()[1].role, Role::Tool { .. }));
    }

    #[test]
    fn compaction_keeps_system_and_tail() {
        let mut ctx = ConversationContext::new(5); // max 5 messages
        ctx.set_system_prompt("sys".into());
        for i in 0..10 {
            ctx.add_user(format!("msg-{i}"));
        }
        // Should have system + last 4 user messages
        assert_eq!(ctx.len(), 5);
        assert!(matches!(ctx.messages()[0].role, Role::System));
        assert!(matches!(&ctx.messages()[4].content, MessageContent::Text(t) if t == "msg-9"));
    }

    #[test]
    fn reset_keeps_system_prompt() {
        let mut ctx = ConversationContext::new(100);
        ctx.set_system_prompt("sys".into());
        ctx.add_user("hello".into());
        ctx.add_user("world".into());
        ctx.reset();
        assert_eq!(ctx.len(), 1);
        assert!(matches!(ctx.messages()[0].role, Role::System));
    }

    #[test]
    fn reset_without_system_clears_all() {
        let mut ctx = ConversationContext::new(100);
        ctx.add_user("hello".into());
        ctx.reset();
        assert!(ctx.is_empty());
    }
}
