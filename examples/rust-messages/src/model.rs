#[lightshade::schema]
pub mod store {
    use lightshade::prelude::*;

    pub struct Inbox {
        #[lightshade(primary_key)]
        pub id: i64,
        pub owner: String,
    }

    #[lightshade(skip)]
    pub struct UnreadDigest {
        pub owner: String,
        pub total: i64,
        pub newest: i64,
    }

    #[lightshade(skip)]
    pub struct Summary {
        pub total: i64,
        pub newest: i64,
    }

    pub struct Message {
        #[lightshade(primary_key)]
        pub id: i64,
        pub inbox_id: i64,
        pub sent_at: i64,
        pub is_read: bool,
        pub body: String,
    }

    #[lightshade::operations]
    pub trait Messages {
        #[lightshade::query(
            filter = Message::inbox_id.eq(inbox) & Message::is_read.eq(false),
            order = (Message::sent_at.desc(), Message::id.desc()),
            limit = limit,
        )]
        fn recent_unread(&self, inbox: i64, limit: i64) -> Result<Vec<Message>>;

        #[lightshade::query(
            filter = Message::inbox_id.eq(inbox),
            order = (Message::sent_at.asc(), Message::id.asc()),
            limit = limit,
            offset = skip,
        )]
        fn inbox_page(&self, inbox: i64, limit: i64, skip: i64) -> Result<Vec<Message>>;

        #[lightshade::query(count, filter = Message::inbox_id.eq(inbox) & Message::is_read.eq(false))]
        fn unread_count(&self, inbox: i64) -> Result<i64>;

        #[lightshade::query(
            from = Message,
            select = Summary::select()
                .total(count())
                .newest(max(Message::sent_at)),
            filter = Message::inbox_id.eq(inbox),
        )]
        fn summary(&self, inbox: i64) -> Result<Vec<Summary>>;

        #[lightshade::query(
            from = Message,
            joins = (Inbox.join_on(Message::inbox_id.eq_field(Inbox::id)),),
            filter = Message::inbox_id.eq(inbox) & Message::is_read.eq(false),
            select = UnreadDigest::select()
                .owner(Inbox::owner.select())
                .total(count())
                .newest(max(Message::sent_at)),
            group_by = (Inbox::owner.group(),),
            order = (UnreadDigest::owner.asc(),),
        )]
        fn unread_digest(&self, inbox: i64) -> Result<Vec<UnreadDigest>>;

        #[lightshade::update(
            filter = Message::id.eq(message),
            set = (Message::is_read.set(true),),
        )]
        fn mark_read(&mut self, message: i64) -> Result<u64>;

        #[lightshade::delete(filter = Message::sent_at.lt(cutoff))]
        fn purge_before(&mut self, cutoff: i64) -> Result<u64>;
    }
}
