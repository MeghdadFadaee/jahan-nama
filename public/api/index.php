<?php
function pluck(array $data, string $value, ?string $key = null): array {
    if ($key === null) {
        return array_map(fn($d) => $d[$value] ?? null, $data);
    }

    $result = [];
    foreach ($data as $d) {
        if (isset($d[$key])) {
            $result[$d[$key]] = $d[$value] ?? null;
        }
    }
    return $result;
}

function getInputFields(string $html): array {
    $doc = new DOMDocument();
    @$doc->loadHTML($html);
    $inputs = $doc->getElementsByTagName('input');

    $result = [];
    foreach ($inputs as $input) {
        $name = $input->getAttribute('name');
        $value = $input->getAttribute('value');
        if ($name) {
            $result[$name] = $value;
        }
    }
    return $result;
}

function prettifyLabelText(DOMNode $label): string {
    return trim(trim(trim($label->nodeValue), ':'), " \t\n\r\0\x0B");
}

function getInformation($cookieFile): array {
    $baseurl = 'https://qom.jahan-nama.com';

    // دریافت HTML صفحه
    $ch = curl_init($baseurl);
    curl_setopt_array($ch, [
        CURLOPT_RETURNTRANSFER => true,
        CURLOPT_COOKIEFILE => $cookieFile,
        CURLOPT_COOKIEJAR => $cookieFile,
    ]);
    $html = curl_exec($ch);
    curl_close($ch);

    // تحلیل HTML
    $doc = new DOMDocument();
    @$doc->loadHTML($html);
    $xpath = new DOMXPath($doc);

    $labels = $xpath->query("//div[contains(@class, 'box-info')]//label");

    $info = [];
    for ($i = 0; $i < $labels->length; $i += 2) {
        $key = prettifyLabelText($labels->item($i));
        $val = prettifyLabelText($labels->item($i + 1));
        $info[$key] = $val;
    }

    return $info;
}

function forceToLogin($username, $password, $cookieFile) {
    $baseurl = 'https://qom.jahan-nama.com';
    $loginUri = '/user/signin?isSignout=1';
    $authUri = '/User/SigninCheck';

    // مرحله ۱: دریافت صفحه لاگین
    $ch = curl_init($baseurl . $loginUri);
    curl_setopt_array($ch, [
        CURLOPT_RETURNTRANSFER => true,
        CURLOPT_COOKIEJAR => $cookieFile,
        CURLOPT_COOKIEFILE => $cookieFile,
    ]);
    $loginHtml = curl_exec($ch);
    curl_close($ch);

    // استخراج ورودی‌ها
    $inputs = getInputFields($loginHtml);
    $inputs['Username'] = $username;
    $inputs['Password'] = $password;

    if (isset($inputs['MyIP'])) {
        unset($inputs['MyIP']);
    }

    // مرحله ۲: ارسال اطلاعات لاگین
    $ch = curl_init($baseurl . $authUri);
    curl_setopt_array($ch, [
        CURLOPT_RETURNTRANSFER => true,
        CURLOPT_POST => true,
        CURLOPT_POSTFIELDS => http_build_query($inputs),
        CURLOPT_COOKIEJAR => $cookieFile,
        CURLOPT_COOKIEFILE => $cookieFile,
        CURLOPT_FOLLOWLOCATION => true,
    ]);
    curl_exec($ch);
    curl_close($ch);
}

function json_response(array $data) {
    header('Content-Type: application/json');
    echo json_encode($data, JSON_UNESCAPED_UNICODE | JSON_PRETTY_PRINT);
    exit;
}

function main() {
    // $username = getenv('JAHAN_NAMA_USERNAME');
    // $password = getenv('JAHAN_NAMA_PASSWORD');

    $username = $_GET['username'] ?? null;
    $password = $_GET['password'] ?? null;

    if (empty($username) or empty($password)) {
        http_response_code(422);
        json_response([
            'status' => 1,
            'messages' => 'درخواست مشکلی دارد. موارد را بررسی کنید.',
            'data' => [
               'username' => 'آیتم نام کاربری و رمز عبور الزامی است.',
            ],
        ]);
    }


    $cookieFile = tempnam(sys_get_temp_dir(), 'cookie_');

    forceToLogin($username, $password, $cookieFile);
    $info = getInformation($cookieFile);

    unlink($cookieFile);

    json_response([
        'status' => 0,
        'messages' => '',
        'data' => $info,
    ]);
}

main();
