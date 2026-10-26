#[lightshade::schema]
pub mod catalog {
    use lightshade::prelude::*;

    pub struct Product {
        #[lightshade(primary_key)]
        pub sku: String,
        pub category: i64,
        pub price: i64,
        pub name: String,
    }

    #[lightshade::operations]
    pub trait Catalog {
        #[lightshade::query(filter = Product::sku.eq(wanted), limit = 1)]
        fn get_product(&self, wanted: String) -> Result<Vec<Product>>;

        #[lightshade::query(
            filter = Product::category.eq(wanted),
            order = (Product::price.asc(), Product::sku.asc()),
            limit = limit,
        )]
        fn category_page(&self, wanted: i64, limit: i64) -> Result<Vec<Product>>;

        #[lightshade::query(
            filter = Product::price.le(maximum),
            order = (Product::price.asc(), Product::sku.asc()),
            limit = 50,
        )]
        fn cheap_products(&self, maximum: i64) -> Result<Vec<Product>>;

        #[lightshade::query(count, filter = Product::category.eq(wanted))]
        fn category_count(&self, wanted: i64) -> Result<i64>;

        #[lightshade::query(count, filter = Product::price.ge(0))]
        fn product_count(&self) -> Result<i64>;

        #[lightshade::update(
            filter = Product::sku.eq(wanted_sku),
            set = (Product::price.set(wanted_price),),
        )]
        fn update_price(&mut self, wanted_sku: String, wanted_price: i64) -> Result<u64>;

        #[lightshade::delete(filter = Product::sku.eq(wanted_sku))]
        fn remove_product(&mut self, wanted_sku: String) -> Result<u64>;
    }
}

#[lightshade::schema]
pub mod events {
    use lightshade::prelude::*;

    pub struct Event {
        #[lightshade(primary_key)]
        pub id: i64,
        pub tenant: i64,
        pub ts: i64,
        pub kind: i64,
        pub value: i64,
    }

    #[lightshade::operations]
    pub trait Events {
        #[lightshade::query(
            filter = Event::tenant.eq(wanted) & Event::ts.ge(since),
            order = (Event::ts.asc(), Event::id.asc()),
            limit = 100,
        )]
        fn tenant_window(&self, wanted: i64, since: i64) -> Result<Vec<Event>>;

        #[lightshade::query(
            filter = Event::kind.eq(wanted),
            order = (Event::ts.desc(), Event::id.desc()),
            limit = 50,
        )]
        fn kind_recent(&self, wanted: i64) -> Result<Vec<Event>>;

        #[lightshade::delete(filter = Event::id.eq(event))]
        fn purge_event(&mut self, event: i64) -> Result<u64>;

        #[lightshade::query(count, filter = Event::tenant.eq(wanted))]
        fn tenant_count(&self, wanted: i64) -> Result<i64>;
    }
}

#[lightshade::schema]
pub mod kv {
    use lightshade::prelude::*;

    pub struct Kv {
        #[lightshade(primary_key)]
        pub id: i64,
        pub value: String,
    }

    #[lightshade::operations]
    pub trait KvStore {
        #[lightshade::query(filter = Kv::id.eq(wanted), limit = 1)]
        fn get_kv(&self, wanted: i64) -> Result<Vec<Kv>>;

        #[lightshade::query(
            filter = Kv::id.ge(start),
            order = (Kv::id.asc(),),
            limit = limit,
        )]
        fn scan_kv(&self, start: i64, limit: i64) -> Result<Vec<Kv>>;

        #[lightshade::update(
            filter = Kv::id.eq(wanted),
            set = (Kv::value.set(wanted_value),),
        )]
        fn update_kv(&mut self, wanted: i64, wanted_value: String) -> Result<u64>;

        #[lightshade::query(count, filter = Kv::id.ge(0))]
        fn kv_count(&self) -> Result<i64>;
    }
}

#[lightshade::schema]
pub mod sessions {
    use lightshade::prelude::*;

    pub struct Session {
        #[lightshade(primary_key)]
        pub id: i64,
        pub user: i64,
        pub last_seen: i64,
        pub active: bool,
        pub token: String,
    }

    #[lightshade::operations]
    pub trait Sessions {
        #[lightshade::query(filter = Session::id.eq(session), limit = 1)]
        fn get_session(&self, session: i64) -> Result<Vec<Session>>;

        #[lightshade::update(
            filter = Session::id.eq(session),
            set = (Session::last_seen.set(now),),
        )]
        fn touch_session(&mut self, session: i64, now: i64) -> Result<u64>;

        #[lightshade::delete(filter = Session::id.eq(session))]
        fn end_session(&mut self, session: i64) -> Result<u64>;

        #[lightshade::query(
            count,
            filter = Session::user.eq(wanted) & Session::active.eq(true),
        )]
        fn active_sessions(&self, wanted: i64) -> Result<i64>;
    }
}

#[lightshade::schema]
pub mod order_book {
    use lightshade::prelude::*;

    pub struct Order {
        #[lightshade(primary_key)]
        pub id: i64,
        pub customer: i64,
        pub created_at: i64,
        pub status: i64,
        pub total: i64,
    }

    #[lightshade(skip)]
    pub struct CustomerTotal {
        pub customer: i64,
        pub orders: i64,
        pub spend: i64,
    }

    #[lightshade::operations]
    pub trait OrderBook {
        #[lightshade::query(filter = Order::id.eq(order), limit = 1)]
        fn get_order(&self, order: i64) -> Result<Vec<Order>>;

        #[lightshade::query(
            filter = Order::customer.eq(wanted),
            order = (Order::created_at.desc(), Order::id.desc()),
            limit = limit,
        )]
        fn customer_orders(&self, wanted: i64, limit: i64) -> Result<Vec<Order>>;

        #[lightshade::query(
            filter = Order::status.eq(wanted) & Order::created_at.ge(since),
            order = (Order::created_at.asc(), Order::id.asc()),
            limit = 100,
        )]
        fn status_window(&self, wanted: i64, since: i64) -> Result<Vec<Order>>;

        #[lightshade::query(
            filter = Order::created_at.ge(since),
            order = (Order::created_at.desc(), Order::id.desc()),
            limit = 100,
        )]
        fn recent_orders(&self, since: i64) -> Result<Vec<Order>>;

        #[lightshade::query(
            from = Order,
            select = CustomerTotal::select()
                .customer(Order::customer.select())
                .orders(count())
                .spend(sum(Order::total)),
            filter = Order::customer.eq(wanted),
            group_by = (Order::customer.group(),),
        )]
        fn customer_total(&self, wanted: i64) -> Result<Vec<CustomerTotal>>;

        #[lightshade::query(count, filter = Order::status.eq(wanted))]
        fn status_count(&self, wanted: i64) -> Result<i64>;

        #[lightshade::query(count, filter = Order::id.ge(0))]
        fn order_count(&self) -> Result<i64>;

        #[lightshade::update(
            filter = Order::id.eq(order),
            set = (Order::status.set(wanted_status),),
        )]
        fn mark_status(&mut self, order: i64, wanted_status: i64) -> Result<u64>;

        #[lightshade::delete(filter = Order::id.eq(order))]
        fn delete_order(&mut self, order: i64) -> Result<u64>;
    }
}

#[lightshade::schema]
pub mod inbox_digest {
    use lightshade::prelude::*;

    pub struct Inbox {
        #[lightshade(primary_key)]
        pub id: i64,
        pub owner: String,
    }

    #[lightshade(table = "messages")]
    pub struct Msg {
        #[lightshade(primary_key)]
        pub id: i64,
        pub inbox_id: i64,
        pub sent_at: i64,
        pub is_read: bool,
        pub body: String,
    }

    #[lightshade(skip)]
    pub struct Digest {
        pub owner: String,
        pub total: i64,
        pub newest: i64,
        pub oldest: i64,
        pub span: i64,
    }

    #[lightshade(skip)]
    pub struct Summary {
        pub total: i64,
        pub oldest: i64,
        pub newest: i64,
        pub average: i64,
        pub span: i64,
    }

    #[lightshade(skip)]
    pub struct Excerpt {
        pub id: i64,
        pub owner: String,
        pub body: String,
    }

    #[lightshade::operations]
    pub trait Digests {
        #[lightshade::query(filter = Msg::id.eq(message), limit = 1)]
        fn get_message(&self, message: i64) -> Result<Vec<Msg>>;

        #[lightshade::query(count, filter = Msg::id.ge(0))]
        fn message_count(&self) -> Result<i64>;

        #[lightshade::update(
            filter = Msg::id.eq(message),
            set = (Msg::is_read.set(true),),
        )]
        fn mark_read(&mut self, message: i64) -> Result<u64>;

        #[lightshade::delete(filter = Msg::id.eq(message))]
        fn delete_message(&mut self, message: i64) -> Result<u64>;

        #[lightshade::query(
            from = Msg,
            joins = (Inbox.join_on(Msg::inbox_id.eq_field(Inbox::id)),),
            filter = Msg::is_read.eq(false),
            select = Digest::select()
                .owner(Inbox::owner.select())
                .total(count())
                .newest(max(Msg::sent_at))
                .oldest(min(Msg::sent_at))
                .span(sum(Msg::sent_at)),
            group_by = (Inbox::owner.group(),),
            order = (Digest::total.desc(), Digest::owner.asc()),
        )]
        fn unread_digest(&self) -> Result<Vec<Digest>>;

        #[lightshade::query(
            from = Msg,
            select = Summary::select()
                .total(count())
                .oldest(min(Msg::sent_at))
                .newest(max(Msg::sent_at))
                .average(avg(Msg::sent_at))
                .span(sum(Msg::sent_at)),
            filter = Msg::sent_at.ge(0),
        )]
        fn overall(&self) -> Result<Vec<Summary>>;

        #[lightshade::query(
            from = Msg,
            joins = (Inbox.join_on(Msg::inbox_id.eq_field(Inbox::id)),),
            filter = Msg::sent_at.ge(minimum),
            select = Excerpt::select()
                .id(Msg::id.select())
                .owner(Inbox::owner.select())
                .body(Msg::body.select()),
            order = (Excerpt::owner.asc(), Excerpt::id.desc()),
            limit = 2,
        )]
        fn excerpts(&self, minimum: i64) -> Result<Vec<Excerpt>>;
    }
}
