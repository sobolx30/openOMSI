# Cookie wiązki reflektora: format i jak go przygotować

**Stan:** gra czyta cookie przez nowe słowo kluczowe `[spotlight_cookie]` w `model.cfg` (rozdział 5), w ścieżce Enhanced. Ten dokument opisuje format obrazu, a skrypt `beam_cookie.py` pozwala go tworzyć i sprawdzać bez gry.

Cookie to zwykły obraz w skali szarości, który mówi, **jak mocno lampa świeci w każdym kierunku**. Dla każdego punktu drogi shader policzyłby dwa kąty do lampy (w bok i w pionie), odczytał jasność z obrazu i pomnożył przez zwykłe światło lampy (kolor, intensywność, odległość).

## 1. Format

| Co | Wartość |
| --- | --- |
| Rozmiar | **1024 × 512** pikseli |
| Skala | **0,09375°** na piksel, w obu osiach (kąt, nie rzut na ścianę) |
| Oś pozioma | środkowa kolumna = prosto przed lampą; w prawo = prawo (od −48° do +48°) |
| Oś pionowa | **wiersz 128 = poziom lampy** (0°); górny wiersz to +12°, dolny to −36° |
| Jasność | skala szarości, kodowanie **sRGB** (to, co widzisz w edytorze); biały = szczyt wiązki |
| Plik | PNG, 8 lub 16 bitów, szary (kolor jest traktowany jako szary) |
| Poza obrazem | ciemno (brak światła) |

- **Biały to szczyt, nie bezwzględna jasność.** Ostateczną moc ustawia lampa w `model.cfg` (kolor i zasięg), tak jak dotąd. Cookie mówi tylko, jak światło rozkłada się w kątach.
- **Oś lampy** ustawia `dir` w `[spotlight_2]` (kierunek jazdy). Cookie opisuje wszystko względem niej, więc dla świateł mijania `dir` powinien być poziomy, a lekkie pochylenie (np. 1% w dół) siedzi w samym obrazie jako położenie linii odcięcia.
- Cienkie szczegóły (linia odcięcia) potrzebują rozdzielczości: przy 0,09° na piksel odcięcie o ostrości 0,2° zajmuje dwa piksele, i tak ma wyglądać.

## 2. Jak narysować od zera

1. Wygeneruj punkt wyjścia:

   ```
   python beam_cookie.py generate low cookie_low.png
   python beam_cookie.py generate high cookie_high.png
   python beam_cookie.py generate fog cookie_fog.png
   python beam_cookie.py generate flood cookie_flood.png
   ```

   Mijania da się dostroić: `--cutoff-left -0.6 --cutoff-right 0.9 --kink 15` (wysokość lewego i prawego odcięcia w stopniach i nachylenie "schodka").

2. Otwórz w GIMP-ie, Krita albo Photoshopie i **maluj w zwykłej skali szarości**. Rysujesz to, co widzisz: gdy obraz wygląda dobrze w edytorze, jasności są poprawnie zakodowane.
3. Anatomia świateł mijania, od najważniejszego:
   - **Linia odcięcia:** twarda krawędź, ostra do 1–3 pikseli (0,1–0,3°). Po lewej płaska, po prawej podniesiona ze schodkiem pod kątem około 15° (ruch prawostronny). To odróżnia reflektor od okrągłej plamy.
   - **Gorący punkt:** najjaśniejszy obszar tuż pod linią odcięcia, przesunięty lekko w prawo od osi. To on świeci daleko w głąb drogi.
   - **Tło przed autem:** słabsze, szerokie światło od linii odcięcia w dół, spadające z kątem.
   - **Poświata boczna:** bardzo słaba, szeroka, do ±40°.
4. Wskazówki:
   - Nie przycinaj jasności do bieli poza szczytem. Biały to szczyt, a reszta ma być odpowiednio ciemniejsza (zwykle gorący punkt jest 5–15× jaśniejszy niż tło).
   - Wyrównaj krawędzie rozmyciem o promieniu 1–3 pikseli, nie więcej, bo stracisz ostrość linii.
   - Pracuj na warstwach: odcięcie jako maska, gorący punkt jako rozmyty owal, tło jako gradient.

## 3. Jak zrobić profil ze zdjęcia prawdziwej lampy

Najbardziej autentyczna metoda: sfotografować plamę światła na ścianie.

**Ustawienie**
- Ciemne pomieszczenie lub noc, **biała, matowa, płaska ściana**.
- Samochód (albo sama lampa) ustawiony **prostopadle do ściany**, 5–25 m od niej. Osłoń drugą lampę.
- Aparat na statywie, obiektyw bez zniekształceń (zwykły, nie rybie oko), jak najbliżej osi lampy, prostopadle do ściany.
- Na ścianie rozmieść znaczniki (taśma miernicza albo naklejki co metr), żeby znać skalę.
- Zrób serię naprawdę różnych ekspozycji (gorący punkt wypala się przy ekspozycji na tło). Zdjęcia w **RAW**.

**Obróbka**
1. Scal ekspozycje w jeden obraz **liniowy** (HDR) i zapisz jako **16-bitowy PNG lub TIFF** (np. RawTherapee, Luminance HDR, Hugin). Odejmij tło (zdjęcie bez zapalonej lampy) albo podaj poziom czerni.
2. Zmierz:
   - `--distance`: odległość lampy od ściany w metrach,
   - `--px-per-m`: ile pikseli zdjęcia przypada na metr ściany,
   - `--axis-x`: kolumna zdjęcia, na której ściana leży prosto przed lampą,
   - `--horizon-y`: wiersz zdjęcia na wysokości lampy.
3. Uruchom:

   ```
   python beam_cookie.py from-wall zdjecie.png cookie.png --distance 10 --px-per-m 180 --axis-x 960 --horizon-y 540 --smooth 0.15
   ```

   Skrypt zamienia rzut na ścianę na kąty i koryguje jasność (światło pada na ścianę pod kątem). `--black` odejmuje światło tła, `--smooth` uspokaja szum.
4. Sprawdź wynik (rozdział 4) i dopracuj w edytorze.

**Ograniczenie:** ściana ma skończoną szerokość. Przy 10 m ściana szeroka na 6 m obejmuje tylko ±17°, a reszta będzie ciemna. Szerokie kąty fotografuj z bliższej ściany (np. 4–5 m), a gorący punkt z dalszej, i scal obrazy w edytorze (tryb mieszania "Lighten", każdy po wyskalowaniu jasności).

## 4. Podgląd bez gry

```
python beam_cookie.py preview cookie.png preview.png
```

Skrypt rysuje, jak ten profil oświetliłby drogę z fotela kierowcy (dwie lampy 0,8 m nad ziemią, 1,7 m od siebie; parametry `--lamp-height`, `--lamp-spacing`, `--eye-height`). `--peak` ustawia, ile znaczy biały (domyślnie 26000, jak w mijaniach z presetu). To przybliżenie do oceny kształtu, nie symulacja gry. W podglądzie ciemne półkola u dołu to miejsce tuż przed lampami, a ciemne trójkąty po bokach to kąty poza ±48°, których obraz nie obejmuje.

## 5. Użycie w grze: `[spotlight_cookie]`

W `model.cfg` pojazdu, jak `[spotlight_2]`, ale z obrazem na ostatniej linii:

```
[spotlight_cookie]
0.95            pozycja x
5.95            pozycja y
0.652           pozycja z
0.0             kierunek x (poziomo przed siebie)
1.0             kierunek y
0.0             kierunek z
255             czerwony
255             zielony
233             niebieski
200             zasięg (m)
30              kąt wewnętrzny stożka (cookie go nie używa)
80              kąt zewnętrzny stożka (cookie go nie używa)
lights_fern     zmienna włączająca (0 wyłączone, 1 pełne)
0               flaga: 0 albo brak = bliźniak po drugiej stronie, 1 = jedna lampa
low_beam.png    nazwa obrazu w folderze texture pojazdu
```

- Obraz jest szukany w folderach tekstur pojazdu tak jak `bitmap` w `[light_enh_2]`.
- **Obraz nie jest odbijany**: bliźniak po drugiej stronie ma pozycję i kierunek odbite, ale ten sam obraz w tej samej orientacji, więc asymetryczne mijania (odcięcie wyżej po prawej) są asymetryczne w tę samą stronę przy obu lampach.
- Ramka obrazu to kierunek lampy plus "góra" pojazdu, więc odcięcie przechyla się razem z pochyleniem i przechyłem autobusu.
- Cookie zastępuje stożek: kąty stożka są ignorowane, a zasięg, kolor i zmienna działają jak zwykle. Jasność reguluje kolor lampy, a stała `COOKIE_GAIN` w `enhanced.wgsl` ustala, ile znaczy biel obrazu w porównaniu ze zwykłym stożkiem (domyślnie 14).
- Do 8 różnych obrazów naraz. Lampa, której obrazu nie ma, nie da się wczytać albo której urządzenie nie obsługuje cookies (np. OpenGL), świeci zwykłym stożkiem.
- Cień od świateł pojazdu gracza działa z cookie bez zmian. `OMSI_NO_COOKIES=1` wyłącza cookies.
- W logu gry (`game.log`) szukaj linii `beam cookies ([spotlight_cookie])` (czy włączone) i `beam cookie ... in slot N` (czy obraz się wczytał).

## 6. Pliki w tym folderze

| Plik | Co to |
| --- | --- |
| `beam_cookie.py` | narzędzie (numpy i Pillow): `generate`, `preview`, `from-wall` |
| `cookie_low.png`, `cookie_high.png`, `cookie_fog.png`, `cookie_flood.png` | przykładowe cookie z presetów |
| `preview_low.png`, `preview_high.png`, `preview_fog.png` | ich podglądy na drodze |
