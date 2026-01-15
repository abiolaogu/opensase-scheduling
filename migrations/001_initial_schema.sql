CREATE TABLE IF NOT EXISTS services (id UUID PRIMARY KEY, name VARCHAR(100) NOT NULL, description TEXT, duration_minutes INTEGER NOT NULL, price BIGINT, currency VARCHAR(3) DEFAULT 'NGN', status VARCHAR(50) DEFAULT 'active', created_at TIMESTAMPTZ DEFAULT NOW());
CREATE TABLE IF NOT EXISTS appointments (id UUID PRIMARY KEY, service_id UUID NOT NULL REFERENCES services(id), customer_name VARCHAR(100) NOT NULL, customer_email VARCHAR(255) NOT NULL, customer_phone VARCHAR(50), start_time TIMESTAMPTZ NOT NULL, end_time TIMESTAMPTZ NOT NULL, status VARCHAR(50) DEFAULT 'confirmed', notes TEXT, created_at TIMESTAMPTZ DEFAULT NOW(), updated_at TIMESTAMPTZ DEFAULT NOW());
CREATE TABLE IF NOT EXISTS availability (id UUID PRIMARY KEY, day_of_week INTEGER NOT NULL UNIQUE, start_time TIME NOT NULL, end_time TIME NOT NULL, is_available BOOLEAN DEFAULT TRUE);
CREATE INDEX idx_appointments_time ON appointments(start_time);
CREATE INDEX idx_appointments_status ON appointments(status);
