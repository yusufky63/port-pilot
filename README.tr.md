# Port Pilot

Port Pilot, Windows'taki her yerel geliştirme portunu açıklar: onu hangi süreç dinliyor, hangi projeye, Git worktree'sine, WSL dağıtımına ya da Docker container'ına ait, kim başlattı ve yanlış sürece dokunmadan nasıl durdurulur. Bildirim alanı simgesi olan bir masaüstü uygulaması ve aynı motoru kullanan `portctl` komut satırı aracından oluşur.

[English](README.md) · [Nasıl çalışır](docs/architecture.md) · [CLI başvurusu](docs/cli.md) · [Güvenlik ve gizlilik](docs/security.md) · [SSS](docs/faq.md)

> Durum: `0.1.0-alpha.1`, ilk kilometre taşı ("Her yerel geliştirme portunu açıkla"). Asıl hedef Windows 11; Windows 10'da elden geldiğince çalışır.

## Neye cevap verir

"3000 portunda bir şey var" yalnızca belirtidir. Port Pilot arkasındaki soruları cevaplar:

- **Sahibi kim.** Port, bağlanılan adres ve erişim (yalnızca yerel, tek ağ, tüm arabirimler); PID'si *ve* oluşturulma zamanıyla sahibi olan süreç, programı, komut satırı, kullanıcısı ve onu başlatan üst süreç zinciri.
- **Hangi proje.** Proje klasörü ve Git deposu, dalı, commit'i, commit edilmemiş değişiklikler ve bağlı bir worktree olup olmadığı. Her eşleştirme nasıl yapıldığını söyler: *kesin* (WSL çalışma klasörü, Compose etiketi) ya da *çıkarım* (komut satırındaki bir yol, programın klasörü). Windows'ta başka bir sürecin çalışma klasörünü okuyan genel bir API yok; Port Pilot bilmediğini biliyormuş gibi göstermez.
- **Kim başlattı.** Üst süreç zincirindeki en yakın başlatıcı: Claude, Codex, Gemini CLI, VS Code, Cursor, bir JetBrains IDE'si, bir terminal ya da Windows hizmet yöneticisi.
- **Windows mu, WSL mi, Docker mı.** WSL'in `wslrelay.exe`'si ve Docker Desktop'ın `com.docker.backend.exe`'si başka bir şey için host portu tutar. Port Pilot bunları yönlendirdikleri Linux sürecine (çalışan dağıtımlarda `ss` ile okunur) ya da container'a (Docker Engine API'sinden okunur) bağlar; arkasında bir şey olmayan yönlendiricileri, yani klasik "port is already allocated" durumunu işaretler.
- **Port neden kullanılamıyor.** Windows'un ayırdığı aralıklardaki (Hyper-V, WinNAT, WSL ve Docker bloklar ayırır) bir porta, dinleyen olmasa bile bağlanılamaz. `portctl who 3000` ve Tanılama sayfası bunu söyler ve yakındaki boş portları önerir.
- **Çakışmalar.** Aynı portu farklı adreslerde dinleyen iki sahip (örneğin `127.0.0.1:3000` ve `[::1]:3000`); hangisinin yanıt vereceği IPv4 mü IPv6 mı kullanıldığına bağlıdır.

## Güvenli durdurma

1. **Önce kimlik.** Bir durdurma isteği kullanıcının gördüğü dinleyiciyi adlandırır. Motor süreci açar ve oluşturulma zamanını gösterilenle karşılaştırır; Windows PID'leri yeniden kullanır, uyuşmazlık her şeyi durdurur. Tutamaç sonuna kadar açık kalır, bu sırada PID yeniden kullanılamaz.
2. **Önce nazik istek.** Pencereli programlara `WM_CLOSE` gider. Konsol programlarına CTRL+C, yok sayılırsa CTRL+BREAK gönderilir; ama yalnızca konsolu hedef, alt süreçleri, paket çalıştırıcısı ve onları başlatan kabuk dışında hiçbir şey kullanmıyorsa. Konsolu bir editör, bir ajan ya da ilgisiz bir program paylaşıyorsa sinyal gönderilmez.
3. **Zorla sonlandırma ayrı bir karardır.** Süreç bekleme süresinden sonra hâlâ çalışıyorsa uygulama yeniden sorar ve zorla sonlandırmanın ne demek olduğunu açıklar; CLI'da `--force` (terminal yoksa `--yes` de) gerekir.
4. **Asla** sistem süreçleri, Windows hizmetleri, WSL ya da Docker yönlendiricileri veya Port Pilot'un kendisi. WSL süreçlerine aynı başlama zamanı kontrolünden sonra dağıtım içinde SIGTERM gönderilir; container'lar Docker API'si üzerinden durdurulur.

## Gizlilik

Port Pilot hiçbir yere bir şey göndermez: telemetri, çökme raporu, güncelleme kontrolü ya da hesap yok. Komut satırları token, parola ve anahtarlar maskelenerek gösterilir ve diske yazılmaz. Ortam değişkenleri hiç okunmaz. Ayrıntılar [docs/security.md](docs/security.md) dosyasında.

## Komut satırı

```powershell
portctl list                     # sahip, proje, ortam ve erişimle dinleyiciler
portctl who 3000                 # bir portu açıklar, yakındaki boş portlarla
portctl tree 5173                # komut satırlarıyla bütün üst süreç zinciri
portctl stop 3000                # nazik durdurma; kapanmazsa çıkış kodu 7
portctl stop 3000 --force --yes  # bekleme süresinden sonra sonlandır, sormadan
portctl find --range 3000-3999   # ayrılmış aralıklara göre kontrol edilmiş boş portlar
portctl doctor                   # Windows, Git, WSL, Docker, ayrılmış aralıklar
portctl projects                 # depo ve worktree'ye göre gruplanmış dinleyiciler
portctl list --json              # sürümlü JSON (schemaVersion 1)
```

Çıkış kodları sabittir: 0 başarılı, 2 geçersiz argüman, 3 dinleyici yok, 4 sahibi belirsiz, 5 erişim reddedildi ya da reddedildi, 6 kimlik değişti, 7 nazik istek zaman aşımı, 8 WSL ya da Docker'a ulaşılamadı, 10 iç hata. Bkz. [docs/cli.md](docs/cli.md).

## Kurulum

Port Pilot henüz yayımlanmadı. Bir `v*` etiketi gönderildiğinde yayın iş akışı şunları üretir:

| Dosya | Kimin için |
|---|---|
| `Port-Pilot_<sürüm>_x64-setup.exe` | Yönetici izni istemeden geçerli kullanıcıya kurar. İngilizce ya da Türkçe kurulum. |
| `Port-Pilot_<sürüm>_x64_portable.zip` | Kurulum yok. Ayarlar exe'nin yanındaki `data` klasöründe durur. `portctl.exe` de içindedir. |
| `SHA256SUMS.txt` | Her dosyanın sağlama toplamı. |

Derlemeler henüz kod imzalı değil; SmartScreen uyarı gösterebilir. İkisi de Windows 10 ve 11'de zaten bulunan WebView2 çalışma zamanını kullanır.

### Kaynaktan derleme

Rust (stable, MSVC), Node.js 22+, pnpm 9, Git ve Visual Studio C++ derleme araçları kurulu Windows 10 ya da 11.

```powershell
pnpm install
pnpm app:dev                 # anında yenilenen masaüstü uygulaması
cargo run -p portpilot-cli -- list
```

## Lisans

[Apache Lisansı 2.0](LICENSE-APACHE) ya da [MIT lisansı](LICENSE-MIT), tercihinize göre.
