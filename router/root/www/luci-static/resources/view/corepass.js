'use strict';
'require view';
'require rpc';
'require ui';
'require poll';

var getStatus=rpc.declare({object:'corepass',method:'status',expect:{}});
var getConfig=rpc.declare({object:'corepass',method:'get_config',expect:{}});
var saveConfig=rpc.declare({object:'corepass',method:'save',params:['subscription','devices','lan_interface'],expect:{}});
var start=rpc.declare({object:'corepass',method:'start',expect:{}});
var stop=rpc.declare({object:'corepass',method:'stop',expect:{}});
var check=rpc.declare({object:'corepass',method:'check',expect:{}});
var logs=rpc.declare({object:'corepass',method:'logs',expect:{}});
var hints=rpc.declare({object:'luci-rpc',method:'getHostHints',expect:{}});

function result(v){if(v&&v.error)throw new Error(v.error);return v||{};}
function notice(text,type){ui.addNotification(null,E('p',{},[text]),type||'info');}

return view.extend({
 load:function(){return Promise.all([getStatus().then(result),getConfig().then(result),hints().catch(function(){return {};})]);},
 render:function(data){
  var self=this,status=data[0],cfg=data[1],hosts=data[2],busy=false;
  var subscription=E('input',{type:'password',class:'cbi-input-text',autocomplete:'new-password',placeholder:cfg.configured?'Подписка сохранена. Оставьте поле пустым, чтобы сохранить её.':'https://ваш-домен/подписка?format=json',style:'width:100%'});
  var devices=E('textarea',{class:'cbi-input-text',rows:4,style:'width:100%;font-family:monospace',placeholder:'AA:BB:CC:DD:EE:FF\nПо одному MAC-адресу на строку'},[(cfg.devices||[]).join('\n')]);
  var lan=E('input',{type:'text',class:'cbi-input-text',value:cfg.lan_interface||'br-lan'});
  var running=E('strong',{},['Загрузка…']),exit=E('strong',{},['—']),memory=E('strong',{},['—']),conflicts=E('div',{}),logbox=E('pre',{class:'cp-log'},['Журнал появится после запуска службы.']);
  var controls=[];
  function update(s){status=s;running.textContent=s.running?'Служба запущена':'Выключено';running.style.color=s.running?'#4d9a42':'inherit';
   exit.textContent=s.check&&s.check.exit_ip?s.check.exit_ip+' · '+s.check.latency_ms+' мс':'Пока не проверен';memory.textContent=Math.round((s.available_memory_kib||0)/1024)+' МБ свободно';
   conflicts.replaceChildren();if((s.other_routing||[]).length)conflicts.appendChild(E('div',{class:'cp-notice'},['Обнаружена другая маршрутизация: '+s.other_routing.join(', ')+'. Corepass её не выключает. Для первого теста остановите соответствующий VPN-раздел в Zapret Manager; настройки сохранятся.']));
   controls.forEach(function(b){b.disabled=busy;});}
  function action(fn){return function(){if(busy)return;busy=true;update(status);Promise.resolve().then(fn).then(result).then(function(v){if(v.message)notice(v.message);return getStatus();}).then(result).then(update).catch(function(e){notice(e.message||String(e),'error');}).finally(function(){busy=false;update(status);});};}
  function button(text,fn,primary){var b=E('button',{class:'cbi-button '+(primary?'cbi-button-action':'cbi-button-neutral'),click:action(fn)},[text]);controls.push(b);return b;}
  var hostSelect=E('select',{class:'cbi-input-select',style:'min-width:260px'},[E('option',{value:''},['Выберите устройство из домашней сети'])]);
  Object.keys(hosts||{}).sort().forEach(function(mac){var h=hosts[mac]||{},ips=h.ipaddrs||h.ipv4addrs||[];hostSelect.appendChild(E('option',{value:mac},[(h.name||mac)+(ips.length?' · '+ips[0]:'')]));});
  var addDevice=E('button',{class:'cbi-button cbi-button-add',click:function(){var mac=hostSelect.value;if(mac){var lines=devices.value.split(/\s+/).filter(Boolean);if(lines.indexOf(mac.toUpperCase())<0)lines.push(mac.toUpperCase());devices.value=lines.join('\n');}}},['Добавить']);
  var root=E('div',{class:'cp-root'},[
   E('style',{},['.cp-root{max-width:1100px}.cp-head{display:flex;justify-content:space-between;align-items:center;gap:20px;margin:15px 0 25px}.cp-head h2{margin:0}.cp-tag{font-size:12px;padding:5px 10px;border:1px solid #719d60;border-radius:20px}.cp-cards{display:grid;grid-template-columns:repeat(3,1fr);gap:14px;margin:20px 0}.cp-card{border:1px solid #89978855;border-radius:10px;padding:20px}.cp-card>span{display:block;opacity:.65;font-size:12px;margin-bottom:12px}.cp-card strong{font-size:17px}.cp-notice{padding:14px 18px;border:1px solid #ad9a5855;background:#a78a1f12;border-radius:8px;margin:15px 0;line-height:1.7}.cp-field{margin:20px 0}.cp-field label{display:block;font-weight:600;margin-bottom:9px}.cp-field small{display:block;opacity:.65;margin-top:8px;line-height:1.6}.cp-actions{display:flex;flex-wrap:wrap;gap:10px;margin:20px 0}.cp-log{white-space:pre-wrap;overflow-wrap:anywhere;max-height:350px;overflow:auto;font-size:11px;padding:18px;border-radius:7px;background:#101c1510}.cp-host{display:flex;gap:8px;margin-bottom:10px;flex-wrap:wrap}@media(max-width:700px){.cp-cards{grid-template-columns:1fr}.cp-head{align-items:flex-start}}']),
   E('div',{class:'cp-head'},[E('div',{},[E('h2',{},['Corepass']),E('p',{},['Личный VPN для выбранных устройств домашней сети'])]),E('span',{class:'cp-tag'},['Rust · '+(status.version||'0.3.0')])]),
   E('div',{class:'cp-notice'},['Модуль устанавливается выключенным. Сначала выберите одно тестовое устройство. Остальные устройства и настройки Zapret Manager автоматически не меняются.']),
   conflicts,
   E('div',{class:'cp-cards'},[E('div',{class:'cp-card'},[E('span',{},['СОСТОЯНИЕ']),running]),E('div',{class:'cp-card'},[E('span',{},['ПРОВЕРЕННЫЙ ВЫХОД VPN']),exit]),E('div',{class:'cp-card'},[E('span',{},['ДОСТУПНАЯ ОЗУ']),memory])]),
   E('div',{class:'cp-actions'},[button('Включить для выбранных устройств',function(){return start();},true),button('Выключить Corepass',function(){return stop();}),button('Проверить выход',function(){return check();})]),
   E('div',{class:'cp-card'},[
    E('h3',{},['Подключение и устройства']),
    E('div',{class:'cp-field'},[E('label',{},['HTTPS-подписка из S-UI']),subscription,E('small',{},['Используйте JSON-подписку с одним Naive HTTPS сервером. Личная ссылка не отображается в статусе и журнале.'])]),
    E('div',{class:'cp-field'},[E('label',{},['Устройства через VPN']),E('div',{class:'cp-host'},[hostSelect,addDevice]),devices,E('small',{},['MAC-адрес можно посмотреть в списке клиентов OpenWrt. Пустой список не включает VPN на всю сеть. Для Wi-Fi используйте MAC, который устройство использует именно в вашей сети.'])]),
    E('div',{class:'cp-field'},[E('label',{},['LAN-интерфейс']),lan,E('small',{},['Обычно br-lan. Меняйте только если домашняя сеть находится на другом мосту.'])]),
    button('Сохранить и проверить настройки',function(){return saveConfig(subscription.value,devices.value.split(/[\s,;]+/).filter(Boolean),lan.value).then(function(v){result(v);subscription.value='';subscription.placeholder='Подписка сохранена';return v;});},true)
   ]),
   E('div',{class:'cp-card',style:'margin-top:20px'},[E('h3',{},['Журнал Corepass']),button('Обновить журнал',function(){return logs().then(function(v){result(v);logbox.textContent=v.log||'Событий Corepass пока нет.';return {};});}),logbox]),
   E('p',{style:'margin-top:18px;opacity:.65;font-size:12px'},['Проверка выхода проверяет локальный VPN-прокси. После включения отдельно проверьте внешний IP на выбранном устройстве. Загрузки и объём памяти на вашем роутере требуют реального теста.'])
  ]);
  update(status);poll.add(function(){if(!busy)return getStatus().then(result).then(update).catch(function(){});},5);
  return root;
 },
 handleSaveApply:null,handleSave:null,handleReset:null
});
